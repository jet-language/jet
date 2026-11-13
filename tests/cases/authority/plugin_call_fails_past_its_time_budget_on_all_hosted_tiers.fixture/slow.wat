;; `crawl` calls `inner`, whose loop fills 64 KiB of memory on every
;; iteration. That spends one unit of fuel per iteration, so the call runs past
;; its 1-second time budget long before its 10,000,000 units of fuel run out.
;; `ping` returns 7.
(component
  (core module $m
    (memory (export "memory") 1)
    (func $inner (result i64)
      (loop
        (memory.fill (i32.const 0) (i32.const 0) (i32.const 65536))
        (br 0))
      (i64.const 0))
    (func $crawl (export "crawl") (result i64)
      (call $inner))
    (func $ping (export "ping") (result i64)
      (i64.const 7)))
  (core instance $i (instantiate $m))
  (type $t (func (result s64)))
  (func $crawl (type $t)
    (canon lift (core func $i "crawl")))
  (func $ping (type $t)
    (canon lift (core func $i "ping")))
  (export "crawl" (func $crawl))
  (export "ping" (func $ping)))
