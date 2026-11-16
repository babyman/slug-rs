/* nftw and mkdtemp are XSI; CLOCK and timespec fields need the rest. Shims
 * build with -std=c11. */
#define _XOPEN_SOURCE 700
#if defined(__APPLE__)
#define _DARWIN_C_SOURCE
#endif

#include "slug_io_fs.h"

#include <errno.h>
#include <fcntl.h>
#include <ftw.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <dirent.h>
#include <unistd.h>

/* Bounds native memory per line and per read. */
#define MAX_LINE_BYTES ((size_t)16u * 1024u * 1024u)
#define MAX_READ_BYTES ((size_t)16u * 1024u * 1024u)
#define READ_CHUNK ((size_t)64u * 1024u)

struct slug_fs_file {
  /* One for the VM until it releases the file, one per queued job. */
  atomic_int references;
  int descriptor;
  /* Bytes read ahead for read_line, at [start, end) of buffer. */
  uint8_t *buffer;
  size_t start;
  size_t end;
  size_t capacity;
};

typedef enum {
  JOB_OPEN,
  JOB_READ,
  JOB_READ_LINE,
  JOB_WRITE,
  JOB_SYNC,
  JOB_CLOSE,
  JOB_STAT,
  JOB_LIST,
  JOB_MAKE_DIRECTORY,
  JOB_RENAME,
  JOB_REMOVE,
  JOB_REMOVE_ALL,
  JOB_TEMPORARY_DIRECTORY,
} job_kind;

typedef struct job {
  job_kind kind;
  slug_fs_file *file;
  char *path;
  char *other;
  int64_t number;
  uint8_t *data;
  size_t length;
  slug_producer *reply;
  struct job *next;
} job;

static pthread_once_t start_once = PTHREAD_ONCE_INIT;
static bool started = false;
static pthread_mutex_t queue_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t queue_ready = PTHREAD_COND_INITIALIZER;
static job *queue_head = NULL;
static job *queue_tail = NULL;

/* ---- Files ---------------------------------------------------------- */

static void file_unref(slug_fs_file *file) {
  if (atomic_fetch_sub(&file->references, 1) == 1) {
    free(file->buffer);
    free(file);
  }
}

/* ---- Replies -------------------------------------------------------- */

static const char *error_code(int error) {
  switch (error) {
    case ENOENT: return "not_found";
    case EEXIST: return "exists";
    case EACCES:
    case EPERM: return "permission";
    case ENOTDIR: return "not_directory";
    case EISDIR: return "is_directory";
    case ENOTEMPTY: return "not_empty";
    case EXDEV: return "cross_device";
    case ENOSPC: return "no_space";
    case EROFS: return "read_only";
    case EMFILE:
    case ENFILE: return "too_many_open";
    case ENAMETOOLONG: return "name_too_long";
    case ELOOP: return "link_loop";
    case EBADF: return "wrong_mode";
    case EINVAL: return "invalid";
    case EBUSY: return "busy";
    default: return "io";
  }
}

static const char *error_message(int error) {
  switch (error) {
    case ENOENT: return "no such file or directory";
    case EEXIST: return "it already exists";
    case EACCES:
    case EPERM: return "permission denied";
    case ENOTDIR: return "a part of the path is not a directory";
    case EISDIR: return "it is a directory";
    case ENOTEMPTY: return "the directory is not empty";
    case EXDEV: return "it is on another filesystem";
    case ENOSPC: return "no space left on the device";
    case EROFS: return "the filesystem is read-only";
    case EMFILE:
    case ENFILE: return "too many files are open";
    case ENAMETOOLONG: return "the name is too long";
    case ELOOP: return "too many symbolic links";
    case EBADF: return "the file is not open for that";
    case EINVAL: return "invalid argument";
    case EBUSY: return "it is in use";
    case EIO: return "input/output error";
    default: return "the system reported an error";
  }
}

static void reply_items(job *work, const slug_producer_item *items, size_t count) {
  slug_producer_send_list(work->reply, items, count);
}

static void reply_status(job *work, int32_t status) {
  slug_producer_item item = slug_producer_item_i64(status);
  reply_items(work, &item, 1);
}

static void reply_system(job *work, int error) {
  const char *code = error_code(error);
  const char *message = error_message(error);
  slug_producer_item items[4];
  items[0] = slug_producer_item_i64(SLUG_FS_SYSTEM);
  items[1] = slug_producer_item_i64(error);
  items[2] = slug_producer_item_text(code, strlen(code));
  items[3] = slug_producer_item_text(message, strlen(message));
  reply_items(work, items, 4);
}

static void reply_number(job *work, int64_t value) {
  slug_producer_item items[2];
  items[0] = slug_producer_item_i64(SLUG_FS_OK);
  items[1] = slug_producer_item_i64(value);
  reply_items(work, items, 2);
}

static void reply_text(job *work, const char *text, size_t length) {
  slug_producer_item items[2];
  items[0] = slug_producer_item_i64(SLUG_FS_OK);
  items[1] = slug_producer_item_text(text, length);
  if (slug_producer_send_list(work->reply, items, 2) == SLUG_PRODUCER_INVALID) {
    reply_status(work, SLUG_FS_LINE_NOT_UTF8);
  }
}

/* ---- File work ------------------------------------------------------ */

static void run_open(job *work) {
  int flags;
  int descriptor;
  switch (work->number) {
    case SLUG_FS_WRITE: flags = O_WRONLY | O_CREAT | O_TRUNC; break;
    case SLUG_FS_APPEND: flags = O_WRONLY | O_CREAT | O_APPEND; break;
    default: flags = O_RDONLY; break;
  }
  do {
    descriptor = open(work->path, flags | O_CLOEXEC, 0666);
  } while (descriptor < 0 && errno == EINTR);
  if (descriptor < 0) {
    reply_system(work, errno);
    return;
  }
  work->file->descriptor = descriptor;
  reply_status(work, SLUG_FS_OK);
}

/* Reads into the file's buffer after `end`, growing it to hold `wanted` more
 * bytes. Returns the count read, 0 at end of file, -1 with errno, or -2 when
 * memory ran out. */
static ssize_t fill(slug_fs_file *file, size_t wanted) {
  ssize_t count;
  if (file->start > 0 && file->start == file->end) file->start = file->end = 0;
  if (file->capacity - file->end < wanted) {
    if (file->start > 0) {
      memmove(file->buffer, file->buffer + file->start, file->end - file->start);
      file->end -= file->start;
      file->start = 0;
    }
    if (file->capacity - file->end < wanted) {
      size_t capacity = file->end + wanted;
      uint8_t *grown = realloc(file->buffer, capacity);
      if (grown == NULL) return -2;
      file->buffer = grown;
      file->capacity = capacity;
    }
  }
  do {
    count = read(file->descriptor, file->buffer + file->end, wanted);
  } while (count < 0 && errno == EINTR);
  if (count > 0) file->end += (size_t)count;
  return count;
}

static void run_read(job *work) {
  slug_fs_file *file = work->file;
  size_t wanted = (size_t)work->number;
  slug_producer_item items[2];
  if (file->start == file->end) {
    ssize_t count = fill(file, wanted < READ_CHUNK ? READ_CHUNK : wanted);
    if (count == -2) { reply_status(work, SLUG_FS_ALLOC); return; }
    if (count < 0) { reply_system(work, errno); return; }
    if (count == 0) { reply_status(work, SLUG_FS_END); return; }
  }
  if (wanted > file->end - file->start) wanted = file->end - file->start;
  items[0] = slug_producer_item_i64(SLUG_FS_OK);
  items[1] = slug_producer_item_bytes(file->buffer + file->start, wanted);
  file->start += wanted;
  reply_items(work, items, 2);
}

static void run_read_line(job *work) {
  slug_fs_file *file = work->file;
  /* Bytes after `start` known to hold no newline; kept across refills, which
   * keep the bytes' offsets from `start`. */
  size_t scanned = 0;
  for (;;) {
    size_t available = file->end - file->start;
    const uint8_t *text = file->buffer + file->start;
    ssize_t count;
    while (scanned < available && text[scanned] != '\n') {
      if (text[scanned] == '\0') { reply_status(work, SLUG_FS_LINE_HAS_NUL); return; }
      if (scanned == MAX_LINE_BYTES) { reply_status(work, SLUG_FS_LINE_TOO_LONG); return; }
      scanned++;
    }
    if (scanned < available) {
      size_t length = scanned;
      if (length > 0 && text[length - 1] == '\r') length--;
      file->start += scanned + 1;
      { reply_text(work, (const char *)text, length); return; }
    }
    count = fill(file, READ_CHUNK);
    if (count == -2) { reply_status(work, SLUG_FS_ALLOC); return; }
    if (count < 0) { reply_system(work, errno); return; }
    if (count == 0) {
      /* An unterminated final line is still a line. */
      size_t length = available;
      text = file->buffer + file->start;
      if (length == 0) { reply_status(work, SLUG_FS_END); return; }
      if (text[length - 1] == '\r') length--;
      file->start = file->end;
      { reply_text(work, (const char *)text, length); return; }
    }
  }
}

static void run_write(job *work) {
  size_t written = 0;
  while (written < work->length) {
    ssize_t count = write(work->file->descriptor, work->data + written, work->length - written);
    if (count < 0) {
      if (errno == EINTR) continue;
      { reply_system(work, errno); return; }
    }
    written += (size_t)count;
  }
  reply_number(work, (int64_t)written);
}

static void run_sync(job *work) {
  if (fsync(work->file->descriptor) != 0) { reply_system(work, errno); return; }
  reply_status(work, SLUG_FS_OK);
}

static void run_close(job *work) {
  if (work->file->descriptor >= 0) close(work->file->descriptor);
  work->file->descriptor = -1;
}

/* ---- Path work ------------------------------------------------------ */

static int64_t milliseconds(const struct stat *info) {
#if defined(__APPLE__)
  return (int64_t)info->st_mtimespec.tv_sec * 1000 + info->st_mtimespec.tv_nsec / 1000000;
#else
  return (int64_t)info->st_mtim.tv_sec * 1000 + info->st_mtim.tv_nsec / 1000000;
#endif
}

static void run_stat(job *work) {
  struct stat info;
  const char *kind;
  slug_producer_item items[4];
  if (lstat(work->path, &info) != 0) {
    if (errno == ENOENT || errno == ENOTDIR) { reply_status(work, SLUG_FS_END); return; }
    { reply_system(work, errno); return; }
  }
  if (S_ISREG(info.st_mode)) {
    kind = "file";
  } else if (S_ISDIR(info.st_mode)) {
    kind = "directory";
  } else if (S_ISLNK(info.st_mode)) {
    kind = "link";
  } else {
    kind = "other";
  }
  items[0] = slug_producer_item_i64(SLUG_FS_OK);
  items[1] = slug_producer_item_text(kind, strlen(kind));
  items[2] = slug_producer_item_i64((int64_t)info.st_size);
  items[3] = slug_producer_item_i64(milliseconds(&info));
  reply_items(work, items, 4);
}

static int compare_names(const void *left, const void *right) {
  return strcmp(*(char *const *)left, *(char *const *)right);
}

static void run_list(job *work) {
  DIR *directory = opendir(work->path);
  struct dirent *entry;
  char **names = NULL;
  size_t count = 0;
  size_t capacity = 0;
  slug_producer_item *items;
  size_t i;
  int failure = 0;
  if (directory == NULL) { reply_system(work, errno); return; }
  for (;;) {
    errno = 0;
    entry = readdir(directory);
    if (entry == NULL) {
      failure = errno;
      break;
    }
    if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) continue;
    if (count == capacity) {
      size_t grown_capacity = capacity == 0 ? 16 : capacity * 2;
      char **grown = realloc(names, grown_capacity * sizeof *names);
      if (grown == NULL) {
        failure = -1;
        break;
      }
      names = grown;
      capacity = grown_capacity;
    }
    names[count] = strdup(entry->d_name);
    if (names[count] == NULL) {
      failure = -1;
      break;
    }
    count++;
  }
  closedir(directory);
  items = failure == 0 ? malloc((count + 1) * sizeof *items) : NULL;
  if (failure == 0 && items == NULL) failure = -1;
  if (failure == 0) {
    qsort(names, count, sizeof *names, compare_names);
    items[0] = slug_producer_item_i64(SLUG_FS_OK);
    for (i = 0; i < count; i++) items[i + 1] = slug_producer_item_text(names[i], strlen(names[i]));
    if (slug_producer_send_list(work->reply, items, count + 1) == SLUG_PRODUCER_INVALID) {
      /* A name that is not UTF-8. */
      reply_system(work, EINVAL);
    }
  } else if (failure < 0) {
    reply_status(work, SLUG_FS_ALLOC);
  } else {
    reply_system(work, failure);
  }
  free(items);
  for (i = 0; i < count; i++) free(names[i]);
  free(names);
}

static bool is_directory(const char *path) {
  struct stat info;
  return stat(path, &info) == 0 && S_ISDIR(info.st_mode);
}

static void run_make_directory(job *work) {
  if (work->number != 0) {
    /* Each parent in turn, then the directory itself. */
    char *cursor = work->path;
    while (*cursor == '/') cursor++;
    for (; *cursor != '\0'; cursor++) {
      if (*cursor != '/') continue;
      *cursor = '\0';
      if (mkdir(work->path, 0777) != 0 && !(errno == EEXIST && is_directory(work->path))) {
        int error = errno;
        *cursor = '/';
        { reply_system(work, error); return; }
      }
      *cursor = '/';
    }
  }
  if (mkdir(work->path, 0777) != 0 &&
      !(work->number != 0 && errno == EEXIST && is_directory(work->path))) {
    { reply_system(work, errno); return; }
  }
  reply_status(work, SLUG_FS_OK);
}

static void run_rename(job *work) {
  if (rename(work->path, work->other) != 0) { reply_system(work, errno); return; }
  reply_status(work, SLUG_FS_OK);
}

static void run_remove(job *work) {
  if (remove(work->path) != 0) { reply_system(work, errno); return; }
  reply_status(work, SLUG_FS_OK);
}

/* nftw takes no context; only the I/O thread walks. */
static int remove_failure = 0;

static int remove_entry(const char *path, const struct stat *info, int kind,
                        struct FTW *walk) {
  (void)info;
  (void)walk;
  if (kind == FTW_NS && errno == ENOENT) return 0;
  if (remove(path) != 0 && errno != ENOENT) {
    remove_failure = errno;
    return 1;
  }
  return 0;
}

static void run_remove_all(job *work) {
  struct stat info;
  if (lstat(work->path, &info) != 0) {
    if (errno == ENOENT) { reply_status(work, SLUG_FS_OK); return; }
    { reply_system(work, errno); return; }
  }
  remove_failure = 0;
  if (nftw(work->path, remove_entry, 16, FTW_DEPTH | FTW_PHYS) != 0) {
    { reply_system(work, remove_failure != 0 ? remove_failure : errno); return; }
  }
  reply_status(work, SLUG_FS_OK);
}

static void run_temporary_directory(job *work) {
  const char *base = getenv("TMPDIR");
  size_t base_length;
  size_t prefix_length = strlen(work->path);
  char *template;
  if (base == NULL || *base == '\0') base = "/tmp";
  base_length = strlen(base);
  while (base_length > 1 && base[base_length - 1] == '/') base_length--;
  if (strchr(work->path, '/') != NULL) { reply_system(work, EINVAL); return; }
  template = malloc(base_length + 1 + prefix_length + 7);
  if (template == NULL) { reply_status(work, SLUG_FS_ALLOC); return; }
  memcpy(template, base, base_length);
  template[base_length] = '/';
  memcpy(template + base_length + 1, work->path, prefix_length);
  memcpy(template + base_length + 1 + prefix_length, "XXXXXX", 7);
  if (mkdtemp(template) == NULL) {
    int error = errno;
    free(template);
    { reply_system(work, error); return; }
  }
  reply_text(work, template, strlen(template));
  free(template);
}

/* ---- The I/O thread ------------------------------------------------- */

static void run(job *work) {
  switch (work->kind) {
    case JOB_OPEN: run_open(work); break;
    case JOB_READ: run_read(work); break;
    case JOB_READ_LINE: run_read_line(work); break;
    case JOB_WRITE: run_write(work); break;
    case JOB_SYNC: run_sync(work); break;
    case JOB_CLOSE: run_close(work); break;
    case JOB_STAT: run_stat(work); break;
    case JOB_LIST: run_list(work); break;
    case JOB_MAKE_DIRECTORY: run_make_directory(work); break;
    case JOB_RENAME: run_rename(work); break;
    case JOB_REMOVE: run_remove(work); break;
    case JOB_REMOVE_ALL: run_remove_all(work); break;
    case JOB_TEMPORARY_DIRECTORY: run_temporary_directory(work); break;
  }
}

static void finish(job *work) {
  if (work->reply != NULL) slug_producer_release(work->reply);
  if (work->file != NULL) file_unref(work->file);
  free(work->path);
  free(work->other);
  free(work->data);
  free(work);
}

static void *serve(void *unused) {
  (void)unused;
  for (;;) {
    job *work;
    pthread_mutex_lock(&queue_lock);
    while (queue_head == NULL) pthread_cond_wait(&queue_ready, &queue_lock);
    work = queue_head;
    queue_head = work->next;
    if (queue_head == NULL) queue_tail = NULL;
    pthread_mutex_unlock(&queue_lock);
    run(work);
    finish(work);
  }
  return NULL;
}

static void start(void) {
  pthread_t thread;
  if (pthread_create(&thread, NULL, serve, NULL) == 0) {
    pthread_detach(thread);
    started = true;
  }
}

/* Queues `work`, or, when the I/O thread cannot start, finishes it unsent. */
static int32_t submit(job *work) {
  pthread_once(&start_once, start);
  if (!started) {
    finish(work);
    return SLUG_FS_NOT_QUEUED;
  }
  work->next = NULL;
  pthread_mutex_lock(&queue_lock);
  if (queue_tail == NULL) {
    queue_head = work;
  } else {
    queue_tail->next = work;
  }
  queue_tail = work;
  pthread_cond_signal(&queue_ready);
  pthread_mutex_unlock(&queue_lock);
  return SLUG_FS_OK;
}

/* A job for `file` (which it holds a reference to) and `reply`, or NULL with
 * the reply released. */
static job *new_job(job_kind kind, slug_fs_file *file, slug_producer *reply) {
  job *work = calloc(1, sizeof *work);
  if (work == NULL) {
    if (reply != NULL) slug_producer_release(reply);
    return NULL;
  }
  work->kind = kind;
  work->reply = reply;
  if (file != NULL) {
    atomic_fetch_add(&file->references, 1);
    work->file = file;
  }
  return work;
}

static bool copy_text(char **target, const char *text) {
  *target = strdup(text);
  return *target != NULL;
}

static int32_t submit_path(job_kind kind, const char *path, int64_t number,
                           slug_producer *reply) {
  job *work = new_job(kind, NULL, reply);
  if (work == NULL) return SLUG_FS_NOT_QUEUED;
  work->number = number;
  if (!copy_text(&work->path, path)) {
    finish(work);
    return SLUG_FS_NOT_QUEUED;
  }
  return submit(work);
}

/* ---- Entry points --------------------------------------------------- */

int32_t slug_fs_open(const char *path, int64_t mode, slug_producer *reply, slug_fs_file **file) {
  slug_fs_file *opened = calloc(1, sizeof *opened);
  job *work;
  *file = NULL;
  if (opened == NULL) {
    slug_producer_release(reply);
    return SLUG_FS_NOT_QUEUED;
  }
  atomic_init(&opened->references, 1);
  opened->descriptor = -1;
  work = new_job(JOB_OPEN, opened, reply);
  if (work == NULL) {
    file_unref(opened);
    return SLUG_FS_NOT_QUEUED;
  }
  work->number = mode;
  if (!copy_text(&work->path, path)) {
    finish(work);
    file_unref(opened);
    return SLUG_FS_NOT_QUEUED;
  }
  if (submit(work) != SLUG_FS_OK) {
    file_unref(opened);
    return SLUG_FS_NOT_QUEUED;
  }
  *file = opened;
  return SLUG_FS_OK;
}

int32_t slug_fs_read(slug_fs_file *file, int64_t max_bytes, slug_producer *reply) {
  job *work = new_job(JOB_READ, file, reply);
  if (work == NULL) return SLUG_FS_NOT_QUEUED;
  if (max_bytes < 1) max_bytes = 1;
  if ((uint64_t)max_bytes > MAX_READ_BYTES) max_bytes = (int64_t)MAX_READ_BYTES;
  work->number = max_bytes;
  return submit(work);
}

int32_t slug_fs_read_line(slug_fs_file *file, slug_producer *reply) {
  job *work = new_job(JOB_READ_LINE, file, reply);
  if (work == NULL) return SLUG_FS_NOT_QUEUED;
  return submit(work);
}

int32_t slug_fs_write(slug_fs_file *file, const uint8_t *data, size_t length,
                      slug_producer *reply) {
  job *work = new_job(JOB_WRITE, file, reply);
  if (work == NULL) return SLUG_FS_NOT_QUEUED;
  if (length > 0) {
    work->data = malloc(length);
    if (work->data == NULL) {
      finish(work);
      return SLUG_FS_NOT_QUEUED;
    }
    memcpy(work->data, data, length);
  }
  work->length = length;
  return submit(work);
}

int32_t slug_fs_write_text(slug_fs_file *file, const char *text, size_t length,
                           slug_producer *reply) {
  return slug_fs_write(file, (const uint8_t *)text, length, reply);
}

int32_t slug_fs_sync(slug_fs_file *file, slug_producer *reply) {
  job *work = new_job(JOB_SYNC, file, reply);
  if (work == NULL) return SLUG_FS_NOT_QUEUED;
  return submit(work);
}

void slug_fs_release(slug_fs_file *file) {
  job *work = new_job(JOB_CLOSE, file, NULL);
  /* Without memory for the job the descriptor leaks: closing it here could
   * pull it from under work the I/O thread is doing, and a reused number
   * would then reach another file. */
  if (work != NULL) submit(work);
  file_unref(file);
}

int32_t slug_fs_stat(const char *path, slug_producer *reply) {
  return submit_path(JOB_STAT, path, 0, reply);
}

int32_t slug_fs_list(const char *path, slug_producer *reply) {
  return submit_path(JOB_LIST, path, 0, reply);
}

int32_t slug_fs_make_directory(const char *path, int64_t parents, slug_producer *reply) {
  return submit_path(JOB_MAKE_DIRECTORY, path, parents, reply);
}

int32_t slug_fs_rename(const char *from, const char *to, slug_producer *reply) {
  job *work = new_job(JOB_RENAME, NULL, reply);
  if (work == NULL) return SLUG_FS_NOT_QUEUED;
  if (!copy_text(&work->path, from) || !copy_text(&work->other, to)) {
    finish(work);
    return SLUG_FS_NOT_QUEUED;
  }
  return submit(work);
}

int32_t slug_fs_remove(const char *path, slug_producer *reply) {
  return submit_path(JOB_REMOVE, path, 0, reply);
}

int32_t slug_fs_remove_all(const char *path, slug_producer *reply) {
  return submit_path(JOB_REMOVE_ALL, path, 0, reply);
}

int32_t slug_fs_temporary_directory(const char *prefix, slug_producer *reply) {
  return submit_path(JOB_TEMPORARY_DIRECTORY, prefix, 0, reply);
}
