package main
import "core:fmt"
import "core:mem"
import "core:os"
import "core:reflect"
import "core:slice"
import "core:strings"
import "core:sync"
// For C interop if needed
import "core:c"
import "core:c/libc"
// Custom types for ARC
Arc_Object :: struct {
    ref_count: i32,
    // Data would be embedded or pointed to
    data: rawptr,
}
// Global mutex for thread-safety in ref counting
arc_mutex: sync.Mutex
// Initialize runtime
init_runtime :: proc() {
    fmt.println("HackerScript Runtime Initialized")
}
// ARC Functions
arc_retain :: proc(obj: ^Arc_Object) {
    sync.mutex_lock(&arc_mutex)
    obj.ref_count += 1
    sync.mutex_unlock(&arc_mutex)
}
arc_release :: proc(obj: ^Arc_Object, deallocator: proc(rawptr)) {
    sync.mutex_lock(&arc_mutex)
    obj.ref_count -= 1
    if obj.ref_count <= 0 {
        if obj.data != nil {
            deallocator(obj.data)
        }
        mem.free(cast(rawptr)obj)
    }
    sync.mutex_unlock(&arc_mutex)
}
// Create a new ARC object
arc_new :: proc(T: typeid, deallocator: proc(rawptr) = nil) -> ^Arc_Object {
    obj := new(Arc_Object)
    obj.ref_count = 1
    data, _ := mem.alloc(size_of(T))
    obj.data = data
    return obj
}
// Manual Memory Management (Odin-style friendly)
manual_alloc :: proc(size: int) -> rawptr {
    ptr, _ := mem.alloc(size)
    return ptr
}
manual_free :: proc(ptr: rawptr) {
    mem.free(ptr)
}
// Logging function (corresponds to log"message")
hs_log :: proc(message: string) {
    fmt.printf("%s\n", message)
}
// Example class/struct support (runtime helpers if needed)
hs_class_init :: proc() {
    // Placeholder for class initialization if needed
}
// Main entry for runtime binary (for testing or standalone)
main :: proc() {
    init_runtime()
    // Test ARC
    obj := arc_new(int)
    defer arc_release(obj, manual_free)
    arc_retain(obj)
    arc_release(obj, manual_free) // Still alive
    hs_log("ARC Test: Object retained and released")
    // Test Manual
    ptr := manual_alloc(1024)
    defer manual_free(ptr)
    hs_log("Manual Test: Allocated and freed 1024 bytes")
    // If linked with C, example call
    libc.printf(cstring("C Interop: Hello from libc\n"))
    fmt.println("HackerScript Runtime Test Complete")
}
// Compile this with Odin to a static binary:
// odin build HackerScript-Runtime.odin -out:HackerScript-Runtime -o:speed -no-bounds-check -build-mode:executable -extra-linker-flags:"-static"
