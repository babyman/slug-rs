#include "slug_db_sqlite.h"

#include <limits.h>

int32_t slug_sqlite_open(const char *path, sqlite3 **database) {
  return sqlite3_open_v2(path, database, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE, NULL);
}

bool slug_sqlite_has_statements(sqlite3 *database) {
  return sqlite3_next_stmt(database, NULL) != NULL;
}

int32_t slug_sqlite_prepare(sqlite3 *database, const char *sql, size_t length,
                            sqlite3_stmt **statement) {
  if (length > INT_MAX) return SQLITE_TOOBIG;
  return sqlite3_prepare_v2(database, sql, (int)length, statement, NULL);
}

const char *slug_sqlite_statement_error(sqlite3_stmt *statement) {
  return sqlite3_errmsg(sqlite3_db_handle(statement));
}

int32_t slug_sqlite_statement_changes(sqlite3_stmt *statement) {
  return sqlite3_changes(sqlite3_db_handle(statement));
}

int32_t slug_sqlite_bind_int64(sqlite3_stmt *statement, int32_t index, int64_t value) {
  return sqlite3_bind_int64(statement, index, (sqlite3_int64)value);
}

int32_t slug_sqlite_bind_text(sqlite3_stmt *statement, int32_t index, const char *text,
                              size_t length) {
  if (length > INT_MAX) return SQLITE_TOOBIG;
  return sqlite3_bind_text(statement, index, text, (int)length, SQLITE_TRANSIENT);
}

int32_t slug_sqlite_bind_blob(sqlite3_stmt *statement, int32_t index, const uint8_t *data,
                              size_t length) {
  if (length > INT_MAX) return SQLITE_TOOBIG;
  return sqlite3_bind_blob(statement, index, data, (int)length, SQLITE_TRANSIENT);
}

int64_t slug_sqlite_column_int64(sqlite3_stmt *statement, int32_t column) {
  return (int64_t)sqlite3_column_int64(statement, column);
}

/* A zero-length value still needs a non-NULL pointer, or it would read as nil. */
static const uint8_t empty[1] = {0};

void slug_sqlite_column_text(sqlite3_stmt *statement, int32_t column, const char **text,
                             size_t *length) {
  const unsigned char *value = sqlite3_column_text(statement, column);
  int bytes = sqlite3_column_bytes(statement, column);
  *text = value == NULL ? (const char *)empty : (const char *)value;
  *length = value == NULL || bytes < 0 ? 0u : (size_t)bytes;
}

void slug_sqlite_column_blob(sqlite3_stmt *statement, int32_t column, const uint8_t **data,
                             size_t *length) {
  const void *value = sqlite3_column_blob(statement, column);
  int bytes = sqlite3_column_bytes(statement, column);
  *data = value == NULL ? empty : (const uint8_t *)value;
  *length = value == NULL || bytes < 0 ? 0u : (size_t)bytes;
}
