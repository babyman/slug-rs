#include "slug_ffi_prototype.h"
#include <limits.h>
#include <sqlite3.h>
#include <stdlib.h>
#include <string.h>

typedef struct { sqlite3 *database; } sqlite_database;
typedef struct { sqlite3_stmt *statement; } sqlite_statement;
#define TEXT(value) ((slug_ffi_text){value, sizeof(value) - 1})

static void error(const slug_ffi_host_api *host, slug_ffi_call *call, sqlite3 *database) {
  const char *message = database == NULL ? "cannot open database" : sqlite3_errmsg(database);
  host->set_error(call, TEXT("sqlite.error"), (slug_ffi_text){message, strlen(message)});
}
static void destroy_database(void *raw) {
  sqlite_database *database = raw;
  if (database != NULL) { if (database->database != NULL) sqlite3_close_v2(database->database); free(database); }
}
static void destroy_statement(void *raw) {
  sqlite_statement *statement = raw;
  if (statement != NULL) { if (statement->statement != NULL) sqlite3_finalize(statement->statement); free(statement); }
}
static int sql_arguments(const slug_ffi_host_api *host, slug_ffi_call *call, void **raw, slug_ffi_text *sql) {
  if (!host->argument_resource(call, 0, TEXT("slug.db.sqlite.Database"), raw) || !host->argument_text(call, 1, sql)) return 0;
  if (sql->length > INT_MAX) { host->set_error(call, TEXT("sqlite.error"), TEXT("SQL text is too large")); return 0; }
  return 1;
}
static int bind_values(const slug_ffi_host_api *host, slug_ffi_call *call, sqlite3_stmt *statement, size_t count, size_t first) {
  for (size_t index = first; index < count; index++) {
    slug_ffi_value_kind kind;
    int status;
    if (!host->argument_kind(call, index, &kind)) return 0;
    switch (kind) {
      case SLUG_FFI_VALUE_NIL: status = sqlite3_bind_null(statement, (int)(index - first + 1)); break;
      case SLUG_FFI_VALUE_INT: { int64_t value; if (!host->argument_i64(call, index, &value)) return 0; status = sqlite3_bind_int64(statement, (int)(index - first + 1), value); break; }
      case SLUG_FFI_VALUE_FLOAT: { double value; if (!host->argument_f64(call, index, &value)) return 0; status = sqlite3_bind_double(statement, (int)(index - first + 1), value); break; }
      case SLUG_FFI_VALUE_TEXT: { slug_ffi_text value; if (!host->argument_text(call, index, &value)) return 0; if (value.length > INT_MAX) { host->set_error(call, TEXT("sqlite.error"), TEXT("text parameter is too large")); return 0; } status = sqlite3_bind_text(statement, (int)(index - first + 1), value.data, (int)value.length, SQLITE_TRANSIENT); break; }
      case SLUG_FFI_VALUE_BYTES: { slug_ffi_text value; if (!host->argument_bytes(call, index, &value)) return 0; if (value.length > INT_MAX) { host->set_error(call, TEXT("sqlite.error"), TEXT("blob parameter is too large")); return 0; } status = sqlite3_bind_blob(statement, (int)(index - first + 1), value.data, (int)value.length, SQLITE_TRANSIENT); break; }
      default: host->set_error(call, TEXT("sqlite.bind"), TEXT("unsupported SQL parameter")); return 0;
    }
    if (status != SQLITE_OK) { error(host, call, sqlite3_db_handle(statement)); return 0; }
  }
  return 1;
}
static int32_t open_database(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state; slug_ffi_text path; sqlite3 *raw = NULL; sqlite_database *database; char *filename;
  if (!host->argument_text(call, 0, &path) || path.length > INT_MAX) return SLUG_FFI_ERROR;
  filename = malloc((size_t)path.length + 1);
  if (filename == NULL) { host->set_error(call, TEXT("sqlite.error"), TEXT("cannot allocate database path")); return SLUG_FFI_ERROR; }
  memcpy(filename, path.data, (size_t)path.length); filename[path.length] = '\0';
  if (sqlite3_open_v2(filename, &raw, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE, NULL) != SQLITE_OK) { free(filename); error(host, call, raw); if (raw != NULL) sqlite3_close_v2(raw); return SLUG_FFI_ERROR; }
  free(filename);
  database = malloc(sizeof(*database));
  if (database == NULL) { sqlite3_close_v2(raw); host->set_error(call, TEXT("sqlite.error"), TEXT("cannot allocate database handle")); return SLUG_FFI_ERROR; }
  database->database = raw;
  if (!host->set_resource(call, TEXT("slug.db.sqlite.Database"), database)) { destroy_database(database); return SLUG_FFI_ERROR; }
  return SLUG_FFI_OK;
}
static int32_t exec_sql(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state; void *raw; slug_ffi_text sql; sqlite3_stmt *statement = NULL; sqlite_database *database;
  if (!sql_arguments(host, call, &raw, &sql)) return SLUG_FFI_ERROR; database = raw;
  if (sqlite3_prepare_v2(database->database, sql.data, (int)sql.length, &statement, NULL) != SQLITE_OK) { error(host, call, database->database); return SLUG_FFI_ERROR; }
  if (!bind_values(host, call, statement, (size_t)host->argument_count(call), 2)) { sqlite3_finalize(statement); return SLUG_FFI_ERROR; }
  if (sqlite3_step(statement) != SQLITE_DONE) { sqlite3_finalize(statement); error(host, call, database->database); return SLUG_FFI_ERROR; }
  sqlite3_finalize(statement); host->set_i64(call, sqlite3_changes(database->database)); return SLUG_FFI_OK;
}
static int set_column(const slug_ffi_host_api *host, slug_ffi_call *call, slug_ffi_map *row, sqlite3_stmt *statement, int column) {
  const char *name = sqlite3_column_name(statement, column); slug_ffi_text key = {name, strlen(name)};
  switch (sqlite3_column_type(statement, column)) {
    case SQLITE_NULL: return host->map_set_nil(call, row, key);
    case SQLITE_INTEGER: return host->map_set_i64(call, row, key, sqlite3_column_int64(statement, column));
    case SQLITE_FLOAT: return host->map_set_f64(call, row, key, sqlite3_column_double(statement, column));
    case SQLITE_TEXT: { const unsigned char *value = sqlite3_column_text(statement, column); return host->map_set_text(call, row, key, (slug_ffi_text){(const char *)value, (uint64_t)sqlite3_column_bytes(statement, column)}); }
    case SQLITE_BLOB: { const void *value = sqlite3_column_blob(statement, column); return host->map_set_bytes(call, row, key, (slug_ffi_text){value, (uint64_t)sqlite3_column_bytes(statement, column)}); }
    default: host->set_error(call, TEXT("sqlite.result"), TEXT("unsupported SQLite column type")); return 0;
  }
}
static int32_t query_sql(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state; void *raw; slug_ffi_text sql; sqlite3_stmt *statement = NULL; sqlite_database *database; slug_ffi_list *rows;
  if (!sql_arguments(host, call, &raw, &sql)) return SLUG_FFI_ERROR; database = raw;
  if (sqlite3_prepare_v2(database->database, sql.data, (int)sql.length, &statement, NULL) != SQLITE_OK) { error(host, call, database->database); return SLUG_FFI_ERROR; }
  if (!bind_values(host, call, statement, (size_t)host->argument_count(call), 2)) { sqlite3_finalize(statement); return SLUG_FFI_ERROR; }
  rows = host->list_create(call, 0); if (rows == NULL) { sqlite3_finalize(statement); return SLUG_FFI_ERROR; }
  for (;;) { int status = sqlite3_step(statement); if (status == SQLITE_DONE) break; if (status != SQLITE_ROW) { host->list_destroy(rows); sqlite3_finalize(statement); error(host, call, database->database); return SLUG_FFI_ERROR; }
    int columns = sqlite3_column_count(statement); slug_ffi_map *row = host->map_create(call, (uint64_t)columns); if (row == NULL) { host->list_destroy(rows); sqlite3_finalize(statement); return SLUG_FFI_ERROR; }
    for (int column = 0; column < columns; column++) if (!set_column(host, call, row, statement, column)) { host->map_destroy(row); host->list_destroy(rows); sqlite3_finalize(statement); return SLUG_FFI_ERROR; }
    if (!host->list_append_map(call, rows, row)) { host->map_destroy(row); host->list_destroy(rows); sqlite3_finalize(statement); return SLUG_FFI_ERROR; }
  }
  sqlite3_finalize(statement); if (!host->set_list(call, rows)) return SLUG_FFI_ERROR; return SLUG_FFI_OK;
}
static int32_t close_database(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state; void *raw = NULL; if (!host->argument_resource(call, 0, TEXT("slug.db.sqlite.Database"), &raw)) return SLUG_FFI_ERROR;
  sqlite_database *database = raw; if (sqlite3_close(database->database) != SQLITE_OK) { error(host, call, database->database); return SLUG_FFI_ERROR; }
  database->database = NULL; if (!host->close_resource(call, 0, TEXT("slug.db.sqlite.Database"))) return SLUG_FFI_ERROR; host->set_i64(call, 0); return SLUG_FFI_OK;
}
static int32_t prepare_statement(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state; void *raw; slug_ffi_text sql; sqlite_database *database; sqlite_statement *statement;
  if (!sql_arguments(host, call, &raw, &sql)) return SLUG_FFI_ERROR; database = raw;
  statement = malloc(sizeof(*statement)); if (statement == NULL) { host->set_error(call, TEXT("sqlite.error"), TEXT("cannot allocate statement handle")); return SLUG_FFI_ERROR; }
  statement->statement = NULL;
  if (sqlite3_prepare_v2(database->database, sql.data, (int)sql.length, &statement->statement, NULL) != SQLITE_OK) { error(host, call, database->database); destroy_statement(statement); return SLUG_FFI_ERROR; }
  if (!host->set_resource(call, TEXT("slug.db.sqlite.statement.Statement"), statement)) { destroy_statement(statement); return SLUG_FFI_ERROR; }
  return SLUG_FFI_OK;
}
static int statement_argument(const slug_ffi_host_api *host, slug_ffi_call *call, sqlite_statement **statement) {
  void *raw = NULL; if (!host->argument_resource(call, 0, TEXT("slug.db.sqlite.statement.Statement"), &raw)) return 0; *statement = raw; return 1;
}
static void reset_statement(sqlite_statement *statement) {
  sqlite3_reset(statement->statement);
  sqlite3_clear_bindings(statement->statement);
}
static int32_t exec_statement(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state; sqlite_statement *statement;
  if (!statement_argument(host, call, &statement)) return SLUG_FFI_ERROR;
  if (!bind_values(host, call, statement->statement, (size_t)host->argument_count(call), 1)) { reset_statement(statement); return SLUG_FFI_ERROR; }
  if (sqlite3_step(statement->statement) != SQLITE_DONE) { error(host, call, sqlite3_db_handle(statement->statement)); reset_statement(statement); return SLUG_FFI_ERROR; }
  reset_statement(statement); host->set_i64(call, sqlite3_changes(sqlite3_db_handle(statement->statement))); return SLUG_FFI_OK;
}
static int32_t query_statement(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state; sqlite_statement *statement; slug_ffi_list *rows;
  if (!statement_argument(host, call, &statement)) return SLUG_FFI_ERROR;
  if (!bind_values(host, call, statement->statement, (size_t)host->argument_count(call), 1)) { reset_statement(statement); return SLUG_FFI_ERROR; }
  rows = host->list_create(call, 0); if (rows == NULL) { reset_statement(statement); return SLUG_FFI_ERROR; }
  for (;;) { int status = sqlite3_step(statement->statement); if (status == SQLITE_DONE) break; if (status != SQLITE_ROW) { host->list_destroy(rows); error(host, call, sqlite3_db_handle(statement->statement)); reset_statement(statement); return SLUG_FFI_ERROR; }
    int columns = sqlite3_column_count(statement->statement); slug_ffi_map *row = host->map_create(call, (uint64_t)columns); if (row == NULL) { host->list_destroy(rows); reset_statement(statement); return SLUG_FFI_ERROR; }
    for (int column = 0; column < columns; column++) if (!set_column(host, call, row, statement->statement, column)) { host->map_destroy(row); host->list_destroy(rows); reset_statement(statement); return SLUG_FFI_ERROR; }
    if (!host->list_append_map(call, rows, row)) { host->map_destroy(row); host->list_destroy(rows); reset_statement(statement); return SLUG_FFI_ERROR; }
  }
  reset_statement(statement); if (!host->set_list(call, rows)) return SLUG_FFI_ERROR; return SLUG_FFI_OK;
}
static int32_t close_statement(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state; if (!host->close_resource(call, 0, TEXT("slug.db.sqlite.statement.Statement"))) return SLUG_FFI_ERROR; host->set_i64(call, 0); return SLUG_FFI_OK;
}
static const slug_ffi_function_descriptor DATABASE_FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), TEXT("open"), TEXT("sqlite.open/v1"), 1, 1, open_database},
  {sizeof(slug_ffi_function_descriptor), TEXT("close"), TEXT("sqlite.close/v1"), 1, 1, close_database},
  {sizeof(slug_ffi_function_descriptor), TEXT("exec"), TEXT("sqlite.exec/v1"), 2, UINT64_MAX, exec_sql},
  {sizeof(slug_ffi_function_descriptor), TEXT("query"), TEXT("sqlite.query/v1"), 2, UINT64_MAX, query_sql},
};
static const slug_ffi_function_descriptor STATEMENT_FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), TEXT("prepare"), TEXT("sqlite.prepare/v1"), 2, 2, prepare_statement},
  {sizeof(slug_ffi_function_descriptor), TEXT("close"), TEXT("sqlite.close_statement/v1"), 1, 1, close_statement},
  {sizeof(slug_ffi_function_descriptor), TEXT("exec"), TEXT("sqlite.exec_statement/v1"), 1, UINT64_MAX, exec_statement},
  {sizeof(slug_ffi_function_descriptor), TEXT("query"), TEXT("sqlite.query_statement/v1"), 1, UINT64_MAX, query_statement},
};
static const slug_ffi_resource_descriptor DATABASE_RESOURCES[] = {{sizeof(slug_ffi_resource_descriptor), TEXT("Database"), destroy_database}};
static const slug_ffi_resource_descriptor STATEMENT_RESOURCES[] = {{sizeof(slug_ffi_resource_descriptor), TEXT("Statement"), destroy_statement}};
static const slug_ffi_module_descriptor MODULES[] = {
  {SLUG_FFI_PROTOTYPE_ABI_MAJOR, SLUG_FFI_PROTOTYPE_ABI_MINOR, sizeof(slug_ffi_module_descriptor), TEXT("slug.db.sqlite"), DATABASE_FUNCTIONS, 4, DATABASE_RESOURCES, 1},
  {SLUG_FFI_PROTOTYPE_ABI_MAJOR, SLUG_FFI_PROTOTYPE_ABI_MINOR, sizeof(slug_ffi_module_descriptor), TEXT("slug.db.sqlite.statement"), STATEMENT_FUNCTIONS, 4, STATEMENT_RESOURCES, 1},
};
static const slug_ffi_library_descriptor LIBRARY = {SLUG_FFI_PROTOTYPE_ABI_MAJOR, SLUG_FFI_PROTOTYPE_ABI_MINOR, sizeof(slug_ffi_library_descriptor), NULL, MODULES, 2};
SLUG_FFI_PROTOTYPE_EXPORT const slug_ffi_library_descriptor *slug_ffi_library_init(const slug_ffi_host_api *host, void **out_state) { if (host == NULL || out_state == NULL) return NULL; *out_state = NULL; return &LIBRARY; }
