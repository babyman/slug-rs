#ifndef SLUG_IO_FS_H
#define SLUG_IO_FS_H

#include <stddef.h>
#include <stdint.h>

#include "slug_producer.h"

/* An open file: its descriptor and read buffer, shared by the VM and the I/O
 * thread until both have let it go. */
typedef struct slug_fs_file slug_fs_file;

/* Every operation below queues work for the one I/O thread and returns at
 * once. The thread sends one list through `reply` and releases it: the
 * operation takes ownership of `reply` whatever it returns. A reply begins
 * with a status:
 *
 *   [SLUG_FS_OK, ...results]
 *   [SLUG_FS_END]                  end of file, or nothing at the path
 *   [SLUG_FS_SYSTEM, errno, code, message]
 *   [SLUG_FS_ALLOC]                memory ran out
 *   [SLUG_FS_LINE_TOO_LONG], [SLUG_FS_LINE_HAS_NUL], [SLUG_FS_LINE_NOT_UTF8]
 *
 * `code` names the failure portably, such as "not_found", "exists",
 * "permission", or "not_empty", and `message` describes it. */
#define SLUG_FS_OK 0
#define SLUG_FS_END 1
#define SLUG_FS_SYSTEM 2
#define SLUG_FS_ALLOC 3
#define SLUG_FS_LINE_TOO_LONG 5
#define SLUG_FS_LINE_HAS_NUL 6
#define SLUG_FS_LINE_NOT_UTF8 7

/* The queueing result when the work cannot be queued. Its reply was released
 * unsent. */
#define SLUG_FS_NOT_QUEUED 1

/* slug_fs_open modes. */
#define SLUG_FS_READ 0
#define SLUG_FS_WRITE 1
#define SLUG_FS_APPEND 2

/* Writes a new file for `path` to `file`, then opens it on the I/O thread:
 * for reading, for writing from empty (creating it), or for appending
 * (creating it). The reply is [SLUG_FS_OK] or a failure; after a failure
 * the file is still released as usual. */
int32_t slug_fs_open(const char *path, int64_t mode, slug_producer *reply, slug_fs_file **file);

/* Reads at most `max_bytes`, from 1 to 16 MiB: [SLUG_FS_OK, bytes] or
 * [SLUG_FS_END]. */
int32_t slug_fs_read(slug_fs_file *file, int64_t max_bytes, slug_producer *reply);

/* Reads the next line without its `\n` or a preceding `\r`: [SLUG_FS_OK,
 * text] or [SLUG_FS_END]. A line holds at most 16 MiB of UTF-8 and no NUL. */
int32_t slug_fs_read_line(slug_fs_file *file, slug_producer *reply);

/* Writes all `length` bytes: [SLUG_FS_OK, length]. */
int32_t slug_fs_write(slug_fs_file *file, const uint8_t *data, size_t length,
                      slug_producer *reply);

/* As slug_fs_write, for text. */
int32_t slug_fs_write_text(slug_fs_file *file, const char *text, size_t length,
                           slug_producer *reply);

/* Flushes the file to its device (fsync): [SLUG_FS_OK]. */
int32_t slug_fs_sync(slug_fs_file *file, slug_producer *reply);

/* Lets the VM's reference go: the I/O thread closes the file after the work
 * already queued for it, and frees it. */
void slug_fs_release(slug_fs_file *file);

/* [SLUG_FS_OK, kind, size, modified] for what is at `path`, without
 * following a final symbolic link: kind "file", "directory", "link", or
 * "other", size in bytes, and modified in milliseconds since the Unix
 * epoch. [SLUG_FS_END] when nothing is there. */
int32_t slug_fs_stat(const char *path, slug_producer *reply);

/* [SLUG_FS_OK, names...]: the directory's entries, sorted by byte value,
 * without "." and "..". */
int32_t slug_fs_list(const char *path, slug_producer *reply);

/* Makes the directory, and with `parents` any missing parents; an existing
 * directory is not a failure with `parents`. [SLUG_FS_OK]. */
int32_t slug_fs_make_directory(const char *path, int64_t parents, slug_producer *reply);

/* Renames `from` to `to`, replacing a file there atomically. [SLUG_FS_OK]. */
int32_t slug_fs_rename(const char *from, const char *to, slug_producer *reply);

/* Removes a file, link, or empty directory. [SLUG_FS_OK]. */
int32_t slug_fs_remove(const char *path, slug_producer *reply);

/* Removes `path` and everything under it, never following symbolic links;
 * nothing at `path` is not a failure. [SLUG_FS_OK]. */
int32_t slug_fs_remove_all(const char *path, slug_producer *reply);

/* Makes a new directory, readable only by its owner, named `prefix` and six
 * random characters under $TMPDIR or /tmp: [SLUG_FS_OK, path]. */
int32_t slug_fs_temporary_directory(const char *prefix, slug_producer *reply);

#endif
