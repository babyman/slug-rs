# Private FFI helper SDK

`include/slug_ffi_helpers.h` is a header-only convenience layer over the
private `slug_ffi_prototype` ABI. It is not a released ABI, package API, or
replacement for the primitive header. It exists so ordinary native Clutch
event sources do not each reimplement thread startup, bounded retry, durable
receiver handles, and shutdown coordination.

## Durable async streams

A `slug_ffi_async_stream` represents one native-owned Slug channel receiver.
Create it during library initialization, return its receiver from a foreign
callback with `slug_ffi_async_stream_set_result`, and destroy it from the
library destructor.

```c
static void worker(slug_ffi_async_sender *out, void *context) {
  while (next_event(context)) {
    if (!slug_ffi_async_sender_send_nil(out)) return;
  }
}

stream = slug_ffi_async_stream_create(host, 16, worker, context, destroy_context);

/* In the foreign callback. */
return slug_ffi_async_stream_set_result(stream, call)
    ? SLUG_FFI_OK
    : SLUG_FFI_ERROR;

/* In the library destructor. */
slug_ffi_async_stream_destroy(stream);
```

The first result request creates the channel and starts a detached native
worker. Later requests return another Slug receiver for the same channel. The
helper holds its internal guard while it clones that receiver into the callback
result, so library teardown cannot invalidate the native handle during the
callback.

When the worker returns, the helper closes its producer. It releases the
producer and all helper state immediately if teardown has already started;
otherwise library teardown releases them later. The worker callback must never
retain its sender or use it after returning.

`slug_ffi_async_sender_send_text` transfers its owned buffer only when the
runtime accepts it. It waits while the channel is full and returns false after
the producer is closed, destroying the buffer in either rejection case.
`slug_ffi_async_sender_send_nil` has the same bounded retry and cancellation
behavior. More specialized event sources may use the raw producer operations
when they need a different full-queue policy.

## Limits

The helper can revoke the producer during runtime teardown, but it cannot
interrupt arbitrary operating-system I/O. A worker blocked in a native read
must arrange for its own cancellation or eventually return. The dynamic module
remains loaded while its producer capability survives, so a late worker return
is safe; it is not a license to retain callback contexts or borrowed Slug
values.
