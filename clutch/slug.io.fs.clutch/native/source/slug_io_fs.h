#ifndef SLUG_IO_FS_H
#define SLUG_IO_FS_H

#include <stddef.h>
#include <stdint.h>

/* An open file stream and its line buffer. */
typedef struct slug_fs_file slug_fs_file;

/* slug_fs_read_line status values. */
#define SLUG_FS_OK 0
#define SLUG_FS_EOF 1
#define SLUG_FS_READ_FAILED 2
#define SLUG_FS_LINE_ALLOC_FAILED 3
#define SLUG_FS_LINE_GROW_FAILED 4
#define SLUG_FS_LINE_TOO_LONG 5
#define SLUG_FS_LINE_HAS_NUL 6

/* slug_fs_write failure results. */
#define SLUG_FS_WRITE_FAILED (-1)
#define SLUG_FS_FLUSH_FAILED (-2)

/* Each open returns NULL when the file cannot be opened. */
slug_fs_file *slug_fs_open_read(const char *path);
slug_fs_file *slug_fs_open_write(const char *path);
slug_fs_file *slug_fs_open_append(const char *path);

/* Reads the next line without its `\n` or a preceding `\r`. On SLUG_FS_OK,
 * `line` points into the file's buffer until the next call on the file. */
int32_t slug_fs_read_line(slug_fs_file *file, const char **line);

/* Writes and flushes `length` bytes; returns `length` or a failure result. */
int64_t slug_fs_write(slug_fs_file *file, const char *text, size_t length);

/* Closes the stream and frees the file. */
void slug_fs_close(slug_fs_file *file);

#endif
