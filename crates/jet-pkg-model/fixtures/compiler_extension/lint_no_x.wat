(component
  (core module $m
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 224))
    (data (i32.const 16) "{\22artifacts\22:[],\22findings\22:[{\22message\22:\22prefer y\22,\22rule\22:\22no-x\22,\22severity\22:\22warning\22,\22span_id\22:\22sp1\22}],\22protocol\22:1,\22proposed_edits\22:[]}")
    (data (i32.const 160) "{\22artifacts\22:[],\22findings\22:[],\22protocol\22:1,\22proposed_edits\22:[]}")
    (func $realloc (export "cabi_realloc") (param $ptr i32) (param $old i32) (param $align i32) (param $new i32) (result i32)
      (local $p i32)
      (local.set $p (global.get $heap))
      ;; align-up: (p + align - 1) & ~(align - 1)
      (local.set $p
        (i32.and
          (i32.add (local.get $p) (i32.sub (local.get $align) (i32.const 1)))
          (i32.xor (i32.sub (local.get $align) (i32.const 1)) (i32.const -1))
        )
      )
      (global.set $heap (i32.add (local.get $p) (local.get $new)))
      (local.get $p)
    )
    (func $analyze (export "analyze") (param $in_ptr i32) (param $in_len i32) (result i32)
      (local $cursor i32)
      (local $end i32)
      (local $found i32)
      (local $out_ptr i32)
      (local $out_len i32)
      (local $ret i32)
      (local.set $cursor (local.get $in_ptr))
      (local.set $end (i32.add (local.get $in_ptr) (local.get $in_len)))
      (block $done
        (loop $scan
          (br_if $done
            (i32.gt_u (i32.add (local.get $cursor) (i32.const 10)) (local.get $end))
          )
          ;; Exact encoded symbol fact fragment: `"name":"x"`.
          (if
            (i32.and
              (i64.eq
                (i64.load (local.get $cursor))
                (i64.const 0x223a22656d616e22)
              )
              (i32.eq
                (i32.load16_u offset=8 (local.get $cursor))
                (i32.const 0x2278)
              )
            )
            (then
              (local.set $found (i32.const 1))
              (br $done)
            )
          )
          (local.set $cursor (i32.add (local.get $cursor) (i32.const 1)))
          (br $scan)
        )
      )
      (if (local.get $found)
        (then
          (local.set $out_ptr (i32.const 16))
          (local.set $out_len (i32.const 136))
        )
        (else
          (local.set $out_ptr (i32.const 160))
          (local.set $out_len (i32.const 63))
        )
      )
      (local.set $ret (call $realloc (i32.const 0) (i32.const 0) (i32.const 4) (i32.const 8)))
      (i32.store (local.get $ret) (local.get $out_ptr))
      (i32.store (i32.add (local.get $ret) (i32.const 4)) (local.get $out_len))
      (local.get $ret)
    )
  )
  (core instance $i (instantiate $m))
  (type $t (func (param "snapshot" (list u8)) (result (list u8))))
  (func $analyze (type $t)
    (canon lift
      (core func $i "analyze")
      (memory $i "memory")
      (realloc (func $i "cabi_realloc"))
    )
  )
  (export "analyze" (func $analyze))
)
