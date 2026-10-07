// Raw collection carrier ownership: Object keeps duplicate insertion order,
// getters lend the original element storage, and constructors consume holders.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>

typedef void *Handle;
extern Handle jet_rt_string_from_static(const char *, int64_t);
extern void jet_rt_string_drop(Handle);
extern int64_t jet_rt_string_compare(Handle, Handle);
extern Handle jet_rt_record_new_DataTree_Text(Handle);
extern Handle jet_rt_record_get_DataTree_Text(Handle);
extern Handle jet_rt_record_new_DataTree_Array(Handle);
extern Handle jet_rt_record_get_DataTree_Array(Handle);
extern Handle jet_rt_record_new_DataTree_Object(Handle);
extern Handle jet_rt_record_get_DataTree_Object(Handle);
extern void jet_rt_handle_drop_DataTree(Handle);
extern Handle jet_rt_record_seq_List_Rec_DataTree_new(int64_t);
extern int64_t jet_rt_record_seq_List_Rec_DataTree_len(Handle);
extern Handle jet_rt_record_seq_List_Rec_DataTree_get(Handle, int64_t);
extern void jet_rt_record_seq_List_Rec_DataTree_push(Handle, Handle);
extern Handle jet_rt_record_seq_Entries_Rec_DataTree_new(int64_t);
extern int64_t jet_rt_record_seq_Entries_Rec_DataTree_len(Handle);
extern Handle jet_rt_record_seq_Entries_Rec_DataTree_get(Handle, int64_t, Handle *);
extern void jet_rt_record_seq_Entries_Rec_DataTree_push(Handle, Handle, Handle);
extern Handle jet_rt_record_seq_List_Byte_new(int64_t);
extern int64_t jet_rt_record_seq_List_Byte_len(Handle);
extern int64_t jet_rt_record_seq_List_Byte_get(Handle, int64_t);
extern void jet_rt_record_seq_List_Byte_push(Handle, int64_t);
extern Handle jet_rt_record_new_DataTree_Bytes(Handle);
extern Handle jet_rt_record_get_DataTree_Bytes(Handle);

static Handle text(const char *s, int64_t n) {
    Handle str = jet_rt_string_from_static(s, n);
    Handle node = jet_rt_record_new_DataTree_Text(str);
    jet_rt_string_drop(str);
    return node;
}

int main(void) {
    Handle bytes = jet_rt_record_seq_List_Byte_new(4);
    jet_rt_record_seq_List_Byte_push(bytes, 0);
    jet_rt_record_seq_List_Byte_push(bytes, 127);
    jet_rt_record_seq_List_Byte_push(bytes, 128);
    jet_rt_record_seq_List_Byte_push(bytes, 255);
    Handle binary = jet_rt_record_new_DataTree_Bytes(bytes);
    Handle borrowed_bytes = jet_rt_record_get_DataTree_Bytes(binary);
    assert(borrowed_bytes == jet_rt_record_get_DataTree_Bytes(binary));
    assert(jet_rt_record_seq_List_Byte_len(borrowed_bytes) == 4);
    assert(jet_rt_record_seq_List_Byte_get(borrowed_bytes, 0) == 0);
    assert(jet_rt_record_seq_List_Byte_get(borrowed_bytes, 1) == 127);
    assert(jet_rt_record_seq_List_Byte_get(borrowed_bytes, 2) == 128);
    assert(jet_rt_record_seq_List_Byte_get(borrowed_bytes, 3) == 255);
    jet_rt_handle_drop_DataTree(binary);
    Handle key = jet_rt_string_from_static("same", 4);
    Handle entries = jet_rt_record_seq_Entries_Rec_DataTree_new(2);
    jet_rt_record_seq_Entries_Rec_DataTree_push(entries, key, text("first", 5));
    jet_rt_record_seq_Entries_Rec_DataTree_push(entries, key, text("last", 4));
    Handle object = jet_rt_record_new_DataTree_Object(entries);
    Handle borrowed_entries = jet_rt_record_get_DataTree_Object(object);
    assert(borrowed_entries == jet_rt_record_get_DataTree_Object(object));
    assert(jet_rt_record_seq_Entries_Rec_DataTree_len(borrowed_entries) == 2);
    Handle key0 = NULL, key1 = NULL;
    Handle first = jet_rt_record_seq_Entries_Rec_DataTree_get(borrowed_entries, 0, &key0);
    Handle last = jet_rt_record_seq_Entries_Rec_DataTree_get(borrowed_entries, 1, &key1);
    assert(first != last);
    assert(jet_rt_string_compare(key0, key) == 0);
    assert(jet_rt_string_compare(key1, key) == 0);
    Handle check_key = NULL;
    assert(first == jet_rt_record_seq_Entries_Rec_DataTree_get(borrowed_entries, 0, &check_key));
    jet_rt_string_drop(check_key);
    Handle expected = jet_rt_string_from_static("first", 5);
    Handle rendered = jet_rt_record_get_DataTree_Text(first);
    assert(jet_rt_string_compare(rendered, expected) == 0);
    jet_rt_string_drop(rendered);
    jet_rt_string_drop(expected);
    jet_rt_string_drop(key0);
    jet_rt_string_drop(key1);
    jet_rt_string_drop(key);
    Handle array = jet_rt_record_seq_List_Rec_DataTree_new(1);
    jet_rt_record_seq_List_Rec_DataTree_push(array, object);
    Handle root = jet_rt_record_new_DataTree_Array(array);
    Handle borrowed_array = jet_rt_record_get_DataTree_Array(root);
    assert(jet_rt_record_seq_List_Rec_DataTree_len(borrowed_array) == 1);
    Handle child = jet_rt_record_seq_List_Rec_DataTree_get(borrowed_array, 0);
    assert(child == jet_rt_record_seq_List_Rec_DataTree_get(borrowed_array, 0));
    assert(jet_rt_record_seq_Entries_Rec_DataTree_len(jet_rt_record_get_DataTree_Object(child)) == 2);
    // Borrowed collection/element pointers are never freed independently.
    jet_rt_handle_drop_DataTree(root);
    return 0;
}
