#ifndef JET_SHARED_SOURCE_H
#define JET_SHARED_SOURCE_H
#include <stddef.h>
#include <stdint.h>

/* Internal Source codec ABI; no factory function/type addresses are retained. */
typedef enum {
    JET_SOURCE_WORD = 0, JET_SOURCE_E4 = 1, JET_SOURCE_LIST = 2,
    JET_SOURCE_INT = 3, JET_SOURCE_RECORD = 4, JET_SOURCE_OPTION = 5
} JetSourceKind;
typedef enum {
    JET_SOURCE_ALIAS = 0,
    JET_SOURCE_SCHEMA_PROCESS_MISMATCH = 1, /* SourceSchemaProcessMismatch */
    JET_SOURCE_SCHEMA_WORLD_LEASE_BOUNDARY = 2, /* SourceSchemaWorldLeaseBoundary */
    JET_SOURCE_BORROW_REENTRANT = 3 /* SourceBorrowReentrant */
} JetSourceBorrowStatus;
typedef struct JetSourceSchema JetSourceSchema;
typedef struct { uint64_t offset; const JetSourceSchema *schema; } JetSourceField;
struct JetSourceSchema {
    uint64_t nominal, hash, size, align, kind, count;
    const JetSourceField *fields;
    const JetSourceSchema *child;
    uint64_t aux0, aux1;
};
typedef struct { uintptr_t token, payload; } JetSourceBorrow;
_Static_assert(sizeof(JetSourceSchema) == 80, "Source descriptor ABI");
_Static_assert(sizeof(JetSourceField) == 16, "Source field ABI");
_Static_assert(offsetof(JetSourceSchema, fields) == 48, "Source field table offset");
_Static_assert(offsetof(JetSourceSchema, aux0) == 64, "Source auxiliary offsets");
_Static_assert(sizeof(JetSourceBorrow) == 16, "Source borrowed view ABI");

/* Hash is FNV-1a over the canonical encoding documented in SharedSource.jet.
 * Schema input is a trusted finite acyclic tree emitted by the checked codec.
 * The constructor transfers payload contents and deep-copies the schema.
 * Every allocation and destruction occurs in this segment's canonical runtime.
 * Initializers run once before concurrent use. No cell/pointer crosses a process.
 */
extern uint64_t jet_rt_source_schema_hash(const JetSourceSchema *);
extern void jet_rt_source_data_clone(const void *, void *, const JetSourceSchema *);
extern void jet_rt_source_data_drop(void *, const JetSourceSchema *);
extern uintptr_t jet_rt_native_shared_source_new(void *, const JetSourceSchema *);
extern uintptr_t jet_rt_native_shared_clone(uintptr_t);
extern void jet_rt_native_shared_drop(uintptr_t);
extern void jet_rt_source_shared_init(void);
/* out is unchanged on refusal; successful views are non-Send, scoped aliases.
 * lifetime: Process=0, World=1. Only explicit quiescent lease reset clears World.
 * A stopped edit begin follows the canonical task stop rail and leaves out
 * unread even if status is zero. The bridge checks that rail before decoding
 * any out slot, exactly as for an ordinary stopped native Shared callback.
 */
extern int64_t jet_rt_source_shared_borrow_begin(uintptr_t, const JetSourceSchema *, int64_t, JetSourceBorrow *);
extern int64_t jet_rt_source_shared_edit_begin(uintptr_t, const JetSourceSchema *, int64_t, JetSourceBorrow *);
extern void jet_rt_source_shared_borrow_end(uintptr_t);
#endif
