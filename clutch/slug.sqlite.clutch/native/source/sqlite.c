#include "slug_ffi_prototype.h"
#include <limits.h>
#include <sqlite3.h>
#include <stdlib.h>
#include <string.h>

typedef struct { sqlite3 *database; } sqlite_database;
typedef struct { sqlite3_stmt *statement; } sqlite_statement;

#define TEXT(value) ((slug_ffi_text){value, sizeof(value) - 1})

static void set_sqlite_error(const slug_ffi_host_api *host, slug_ffi_call *call, sqlite3 *database) {
  const char *message = database == NULL ? "cannot open database" : sqlite3_errmsg(database);
  host->set_error(call, TEXT("sqlite.error"), (slug_ffi_text){message, strlen(message)});
}

static void destroy_database(void *raw) {
  sqlite_database *database = raw;
  if (database == NULL) return;
  if (database->database != NULL) sqlite3_close_v2(database->database);
  free(database);
}

static void destroy_statement(void *raw) {
  sqlite_statement *statement = raw;
  if (statement == NULL) return;
  if (statement->statement != NULL) sqlite3_finalize(statement->statement);
  free(statement);
}

static int32_t open_memory(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  sqlite3 *raw = NULL;
  if (sqlite3_open(":memory:", &raw) != SQLITE_OK) {
    set_sqlite_error(host, call, raw);
    if (raw != NULL) sqlite3_close_v2(raw);
    return SLUG_FFI_ERROR;
  }
  sqlite_database *database = malloc(sizeof(*database));
  if (database == NULL) { sqlite3_close_v2(raw); host->set_error(call, TEXT("sqlite.error"), TEXT("cannot allocate database handle")); return SLUG_FFI_ERROR; }
  database->database = raw;
  if (!host->set_resource(call, TEXT("Database"), database)) { destroy_database(database); return SLUG_FFI_ERROR; }
  return SLUG_FFI_OK;
}

static int sql_argument(const slug_ffi_host_api *host, slug_ffi_call *call, void **raw_database, slug_ffi_text *sql) {
  if (!host->argument_resource(call, 0, TEXT("Database"), raw_database) || !host->argument_text(call, 1, sql)) return 0;
  if (sql->length > INT_MAX) { host->set_error(call, TEXT("sqlite.error"), TEXT("SQL text is too large")); return 0; }
  return 1;
}

static int32_t exec_sql(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state; void *raw = NULL; slug_ffi_text sql;
  if (!sql_argument(host, call, &raw, &sql)) return SLUG_FFI_ERROR;
  sqlite_database *database = raw; sqlite3_stmt *statement = NULL;
  if (sqlite3_prepare_v2(database->database, sql.data, (int)sql.length, &statement, NULL) != SQLITE_OK) { set_sqlite_error(host, call, database->database); return SLUG_FFI_ERROR; }
  int status = sqlite3_step(statement); sqlite3_finalize(statement);
  if (status != SQLITE_DONE) { set_sqlite_error(host, call, database->database); return SLUG_FFI_ERROR; }
  host->set_i64(call, sqlite3_changes(database->database)); return SLUG_FFI_OK;
}

static int32_t query_int(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state; void *raw = NULL; slug_ffi_text sql;
  if (!sql_argument(host, call, &raw, &sql)) return SLUG_FFI_ERROR;
  sqlite_database *database = raw; sqlite3_stmt *statement = NULL;
  if (sqlite3_prepare_v2(database->database, sql.data, (int)sql.length, &statement, NULL) != SQLITE_OK) { set_sqlite_error(host, call, database->database); return SLUG_FFI_ERROR; }
  if (sqlite3_step(statement) != SQLITE_ROW) { sqlite3_finalize(statement); host->set_error(call, TEXT("sqlite.result"), TEXT("query did not return an integer row")); return SLUG_FFI_ERROR; }
  int64_t value = sqlite3_column_int64(statement, 0); sqlite3_finalize(statement); host->set_i64(call, value); return SLUG_FFI_OK;
}

static int32_t close_database(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state; void *raw = NULL;
  if (!host->argument_resource(call, 0, TEXT("Database"), &raw)) return SLUG_FFI_ERROR;
  sqlite_database *database = raw;
  if (sqlite3_close(database->database) != SQLITE_OK) { set_sqlite_error(host, call, database->database); return SLUG_FFI_ERROR; }
  database->database = NULL;
  if (!host->close_resource(call, 0, TEXT("Database"))) return SLUG_FFI_ERROR;
  host->set_i64(call, 0); return SLUG_FFI_OK;
}

static int32_t prepare_statement(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state; void *raw = NULL; slug_ffi_text sql;
  if (!sql_argument(host, call, &raw, &sql)) return SLUG_FFI_ERROR;
  sqlite_database *database = raw; sqlite_statement *statement = malloc(sizeof(*statement));
  if (statement == NULL) { host->set_error(call, TEXT("sqlite.error"), TEXT("cannot allocate statement handle")); return SLUG_FFI_ERROR; }
  statement->statement = NULL;
  if (sqlite3_prepare_v2(database->database, sql.data, (int)sql.length, &statement->statement, NULL) != SQLITE_OK) { set_sqlite_error(host, call, database->database); destroy_statement(statement); return SLUG_FFI_ERROR; }
  if (!host->set_resource(call, TEXT("Statement"), statement)) { destroy_statement(statement); return SLUG_FFI_ERROR; }
  return SLUG_FFI_OK;
}

static int32_t bind_int(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state; void *raw = NULL; int64_t index = 0, value = 0;
  if (!host->argument_resource(call, 0, TEXT("Statement"), &raw) || !host->argument_i64(call, 1, &index) || !host->argument_i64(call, 2, &value)) return SLUG_FFI_ERROR;
  if (index < 1 || index > INT_MAX) { host->set_error(call, TEXT("sqlite.bind"), TEXT("parameter index is out of range")); return SLUG_FFI_ERROR; }
  sqlite_statement *statement = raw;
  if (sqlite3_bind_int64(statement->statement, (int)index, value) != SQLITE_OK) { set_sqlite_error(host, call, sqlite3_db_handle(statement->statement)); return SLUG_FFI_ERROR; }
  host->set_i64(call, 0); return SLUG_FFI_OK;
}

static int32_t step_int(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state; void *raw = NULL;
  if (!host->argument_resource(call, 0, TEXT("Statement"), &raw)) return SLUG_FFI_ERROR;
  sqlite_statement *statement = raw;
  if (sqlite3_step(statement->statement) != SQLITE_ROW) { set_sqlite_error(host, call, sqlite3_db_handle(statement->statement)); return SLUG_FFI_ERROR; }
  host->set_i64(call, sqlite3_column_int64(statement->statement, 0)); return SLUG_FFI_OK;
}

static int32_t close_statement(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  if (!host->close_resource(call, 0, TEXT("Statement"))) return SLUG_FFI_ERROR;
  host->set_i64(call, 0); return SLUG_FFI_OK;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), TEXT("openMemory"), TEXT("sqlite.open_memory/v1"), 0, 0, open_memory},
  {sizeof(slug_ffi_function_descriptor), TEXT("exec"), TEXT("sqlite.exec/v1"), 2, 2, exec_sql},
  {sizeof(slug_ffi_function_descriptor), TEXT("queryInt"), TEXT("sqlite.query_int/v1"), 2, 2, query_int},
  {sizeof(slug_ffi_function_descriptor), TEXT("close"), TEXT("sqlite.close/v1"), 1, 1, close_database},
  {sizeof(slug_ffi_function_descriptor), TEXT("prepare"), TEXT("sqlite.prepare/v1"), 2, 2, prepare_statement},
  {sizeof(slug_ffi_function_descriptor), TEXT("bindInt"), TEXT("sqlite.bind_int/v1"), 3, 3, bind_int},
  {sizeof(slug_ffi_function_descriptor), TEXT("stepInt"), TEXT("sqlite.step_int/v1"), 1, 1, step_int},
  {sizeof(slug_ffi_function_descriptor), TEXT("closeStatement"), TEXT("sqlite.close_statement/v1"), 1, 1, close_statement},
};
static const slug_ffi_resource_descriptor RESOURCES[] = {
  {sizeof(slug_ffi_resource_descriptor), TEXT("Database"), destroy_database},
  {sizeof(slug_ffi_resource_descriptor), TEXT("Statement"), destroy_statement},
};
static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR, SLUG_FFI_PROTOTYPE_ABI_MINOR, sizeof(slug_ffi_module_descriptor),
  TEXT("slug.sqlite"), NULL, FUNCTIONS, 8, RESOURCES, 2,
};
SLUG_FFI_PROTOTYPE_EXPORT const slug_ffi_module_descriptor *slug_ffi_module_init(const slug_ffi_host_api *host, void **out_state) {
  if (host == NULL || out_state == NULL) return NULL; *out_state = NULL; return &MODULE;
}
