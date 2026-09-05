#include "slug_ffi_prototype.h"
#include <limits.h>
#include <sqlite3.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
  sqlite3 *database;
} sqlite_database;

typedef struct {
  sqlite3_stmt *statement;
} sqlite_statement;

static void set_sqlite_error(const slug_ffi_host_api *host, slug_ffi_call *call,
                             sqlite3 *database) {
  const char *message = database == NULL ? "cannot open database" : sqlite3_errmsg(database);
  host->set_error(call, (slug_ffi_text){"sqlite.error", 12},
                  (slug_ffi_text){message, strlen(message)});
}

static void destroy_database(void *raw_database) {
  sqlite_database *database = raw_database;
  if (database == NULL) return;
  if (database->database != NULL) sqlite3_close_v2(database->database);
  free(database);
}

static void destroy_statement(void *raw_statement) {
  sqlite_statement *statement = raw_statement;
  if (statement == NULL) return;
  if (statement->statement != NULL) sqlite3_finalize(statement->statement);
  free(statement);
}

static int32_t open_memory(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  sqlite3 *raw_database = NULL;
  if (sqlite3_open(":memory:", &raw_database) != SQLITE_OK) {
    set_sqlite_error(host, call, raw_database);
    if (raw_database != NULL) sqlite3_close_v2(raw_database);
    return SLUG_FFI_ERROR;
  }
  sqlite_database *database = malloc(sizeof(sqlite_database));
  if (database == NULL) {
    sqlite3_close_v2(raw_database);
    host->set_error(call, (slug_ffi_text){"sqlite.error", 12},
                    (slug_ffi_text){"cannot allocate database handle", 31});
    return SLUG_FFI_ERROR;
  }
  database->database = raw_database;
  if (!host->set_resource(call, (slug_ffi_text){"sqlite.db", 9}, database)) {
    destroy_database(database);
    return SLUG_FFI_ERROR;
  }
  return SLUG_FFI_OK;
}

static int32_t exec_sql(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  void *raw_database = NULL;
  slug_ffi_text sql;
  if (!host->argument_resource(call, 0, (slug_ffi_text){"sqlite.db", 9}, &raw_database) ||
      !host->argument_text(call, 1, &sql)) return SLUG_FFI_ERROR;
  if (sql.length > INT_MAX) {
    host->set_error(call, (slug_ffi_text){"sqlite.error", 12},
                    (slug_ffi_text){"SQL text is too large", 21});
    return SLUG_FFI_ERROR;
  }
  sqlite_database *database = raw_database;
  sqlite3_stmt *statement = NULL;
  int status = sqlite3_prepare_v2(database->database, sql.data, (int)sql.length, &statement, NULL);
  if (status != SQLITE_OK) {
    set_sqlite_error(host, call, database->database);
    return SLUG_FFI_ERROR;
  }
  status = sqlite3_step(statement);
  sqlite3_finalize(statement);
  if (status != SQLITE_DONE) {
    set_sqlite_error(host, call, database->database);
    return SLUG_FFI_ERROR;
  }
  host->set_i64(call, sqlite3_changes(database->database));
  return SLUG_FFI_OK;
}

static int32_t query_int(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  void *raw_database = NULL;
  slug_ffi_text sql;
  if (!host->argument_resource(call, 0, (slug_ffi_text){"sqlite.db", 9}, &raw_database) ||
      !host->argument_text(call, 1, &sql)) return SLUG_FFI_ERROR;
  if (sql.length > INT_MAX) {
    host->set_error(call, (slug_ffi_text){"sqlite.error", 12},
                    (slug_ffi_text){"SQL text is too large", 21});
    return SLUG_FFI_ERROR;
  }
  sqlite_database *database = raw_database;
  sqlite3_stmt *statement = NULL;
  int status = sqlite3_prepare_v2(database->database, sql.data, (int)sql.length, &statement, NULL);
  if (status != SQLITE_OK) {
    set_sqlite_error(host, call, database->database);
    return SLUG_FFI_ERROR;
  }
  status = sqlite3_step(statement);
  if (status != SQLITE_ROW) {
    sqlite3_finalize(statement);
    host->set_error(call, (slug_ffi_text){"sqlite.result", 13},
                    (slug_ffi_text){"query did not return an integer row",
                                     strlen("query did not return an integer row")});
    return SLUG_FFI_ERROR;
  }
  int64_t value = sqlite3_column_int64(statement, 0);
  sqlite3_finalize(statement);
  host->set_i64(call, value);
  return SLUG_FFI_OK;
}

static int32_t close_database(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  void *raw_database = NULL;
  if (!host->argument_resource(call, 0, (slug_ffi_text){"sqlite.db", 9}, &raw_database)) {
    return SLUG_FFI_ERROR;
  }
  sqlite_database *database = raw_database;
  if (sqlite3_close(database->database) != SQLITE_OK) {
    set_sqlite_error(host, call, database->database);
    return SLUG_FFI_ERROR;
  }
  database->database = NULL;
  if (!host->close_resource(call, 0, (slug_ffi_text){"sqlite.db", 9})) {
    return SLUG_FFI_ERROR;
  }
  host->set_i64(call, 0);
  return SLUG_FFI_OK;
}

static int32_t prepare_statement(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  void *raw_database = NULL;
  slug_ffi_text sql;
  if (!host->argument_resource(call, 0, (slug_ffi_text){"sqlite.db", 9}, &raw_database) ||
      !host->argument_text(call, 1, &sql)) return SLUG_FFI_ERROR;
  if (sql.length > INT_MAX) {
    host->set_error(call, (slug_ffi_text){"sqlite.error", 12},
                    (slug_ffi_text){"SQL text is too large", 21});
    return SLUG_FFI_ERROR;
  }
  sqlite_database *database = raw_database;
  sqlite_statement *statement = malloc(sizeof(sqlite_statement));
  if (statement == NULL) {
    host->set_error(call, (slug_ffi_text){"sqlite.error", 12},
                    (slug_ffi_text){"cannot allocate statement handle", 32});
    return SLUG_FFI_ERROR;
  }
  statement->statement = NULL;
  if (sqlite3_prepare_v2(database->database, sql.data, (int)sql.length,
                         &statement->statement, NULL) != SQLITE_OK) {
    set_sqlite_error(host, call, database->database);
    destroy_statement(statement);
    return SLUG_FFI_ERROR;
  }
  if (!host->set_resource(call, (slug_ffi_text){"sqlite.statement", 16}, statement)) {
    destroy_statement(statement);
    return SLUG_FFI_ERROR;
  }
  return SLUG_FFI_OK;
}

static int32_t bind_int(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  void *raw_statement = NULL;
  int64_t index = 0;
  int64_t value = 0;
  if (!host->argument_resource(call, 0, (slug_ffi_text){"sqlite.statement", 16}, &raw_statement) ||
      !host->argument_i64(call, 1, &index) || !host->argument_i64(call, 2, &value)) {
    return SLUG_FFI_ERROR;
  }
  if (index < 1 || index > INT_MAX) {
    host->set_error(call, (slug_ffi_text){"sqlite.bind", 11},
                    (slug_ffi_text){"parameter index is out of range", 31});
    return SLUG_FFI_ERROR;
  }
  sqlite_statement *statement = raw_statement;
  if (sqlite3_bind_int64(statement->statement, (int)index, value) != SQLITE_OK) {
    set_sqlite_error(host, call, sqlite3_db_handle(statement->statement));
    return SLUG_FFI_ERROR;
  }
  host->set_i64(call, 0);
  return SLUG_FFI_OK;
}

static int32_t step_int(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  void *raw_statement = NULL;
  if (!host->argument_resource(call, 0, (slug_ffi_text){"sqlite.statement", 16}, &raw_statement)) {
    return SLUG_FFI_ERROR;
  }
  sqlite_statement *statement = raw_statement;
  int status = sqlite3_step(statement->statement);
  if (status != SQLITE_ROW) {
    set_sqlite_error(host, call, sqlite3_db_handle(statement->statement));
    return SLUG_FFI_ERROR;
  }
  host->set_i64(call, sqlite3_column_int64(statement->statement, 0));
  return SLUG_FFI_OK;
}

static int32_t close_statement(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  if (!host->close_resource(call, 0, (slug_ffi_text){"sqlite.statement", 16})) {
    return SLUG_FFI_ERROR;
  }
  host->set_i64(call, 0);
  return SLUG_FFI_OK;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), {"openMemory", 10}, {"sqlite.open_memory/v1", 21}, 0, 0, open_memory},
  {sizeof(slug_ffi_function_descriptor), {"exec", 4}, {"sqlite.exec/v1", 14}, 2, 2, exec_sql},
  {sizeof(slug_ffi_function_descriptor), {"queryInt", 8}, {"sqlite.query_int/v1", 19}, 2, 2, query_int},
  {sizeof(slug_ffi_function_descriptor), {"close", 5}, {"sqlite.close/v1", 15}, 1, 1, close_database},
  {sizeof(slug_ffi_function_descriptor), {"prepare", 7}, {"sqlite.prepare/v1", 17}, 2, 2, prepare_statement},
  {sizeof(slug_ffi_function_descriptor), {"bindInt", 7}, {"sqlite.bind_int/v1", 18}, 3, 3, bind_int},
  {sizeof(slug_ffi_function_descriptor), {"stepInt", 7}, {"sqlite.step_int/v1", 18}, 1, 1, step_int},
  {sizeof(slug_ffi_function_descriptor), {"closeStatement", 14}, {"sqlite.close_statement/v1", 25}, 1, 1, close_statement},
};

static const slug_ffi_resource_descriptor RESOURCES[] = {
  {sizeof(slug_ffi_resource_descriptor), {"sqlite.db", 9}, destroy_database},
  {sizeof(slug_ffi_resource_descriptor), {"sqlite.statement", 16}, destroy_statement},
};

static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR,
  SLUG_FFI_PROTOTYPE_ABI_MINOR,
  sizeof(slug_ffi_module_descriptor),
  {"slug.sqlite", 11},
  NULL,
  FUNCTIONS,
  8,
  RESOURCES,
  2,
};

const slug_ffi_module_descriptor *slug_ffi_module_init(const slug_ffi_host_api *host,
                                                        void **out_state) {
  if (host == NULL || out_state == NULL) return NULL;
  *out_state = NULL;
  return &MODULE;
}
