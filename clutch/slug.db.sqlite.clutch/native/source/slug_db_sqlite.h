#ifndef SLUG_DB_SQLITE_H
#define SLUG_DB_SQLITE_H

/* Thin helpers for the SQLite calls that typed bindings cannot bind directly:
 * out-pointer opens, `size_t` lengths, `SQLITE_TRANSIENT`, `sqlite3_int64`,
 * and `unsigned char` text. Every other binding calls SQLite itself. */

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <sqlite3.h>

/* Opens read-write, creating the file. Returns the SQLite status; the handle
 * may be set even on failure, so its error message can be read. */
int32_t slug_sqlite_open(const char *path, sqlite3 **database);

/* Whether any prepared statement on `database` is not finalized. */
bool slug_sqlite_has_statements(sqlite3 *database);

int32_t slug_sqlite_prepare(sqlite3 *database, const char *sql, size_t length,
                            sqlite3_stmt **statement);
const char *slug_sqlite_statement_error(sqlite3_stmt *statement);
int32_t slug_sqlite_statement_changes(sqlite3_stmt *statement);

int32_t slug_sqlite_bind_int64(sqlite3_stmt *statement, int32_t index, int64_t value);
int32_t slug_sqlite_bind_text(sqlite3_stmt *statement, int32_t index, const char *text,
                              size_t length);
int32_t slug_sqlite_bind_blob(sqlite3_stmt *statement, int32_t index, const uint8_t *data,
                              size_t length);

int64_t slug_sqlite_column_int64(sqlite3_stmt *statement, int32_t column);
/* The text and blob stay owned by SQLite until the next step or reset. */
void slug_sqlite_column_text(sqlite3_stmt *statement, int32_t column, const char **text,
                             size_t *length);
void slug_sqlite_column_blob(sqlite3_stmt *statement, int32_t column, const uint8_t **data,
                             size_t *length);

#endif
