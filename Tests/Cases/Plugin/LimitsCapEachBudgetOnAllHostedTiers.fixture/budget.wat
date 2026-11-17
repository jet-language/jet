;; One guest for every `plugin.Limits` field.
;; `burn(n)` loops n times, spending about n units of fuel; `crawl` loops
;; forever, filling 64 KiB per iteration, so its deadline comes long before
;; its fuel runs out; `grow` adds two pages to its one-page memory; `extend`
;; adds four elements to its one-element table; `ping` returns 7.
(component
  (core module $m
    (memory (export "memory") 1)
    (table 1 funcref)
    (func (export "burn") (param $n i64) (result i64)
      (block $done
        (loop $again
          (br_if $done (i64.le_s (local.get $n) (i64.const 0)))
          (local.set $n (i64.sub (local.get $n) (i64.const 1)))
          (br $again)))
      (i64.const 1))
    (func (export "crawl") (result i64)
      (loop
        (memory.fill (i32.const 0) (i32.const 0) (i32.const 65536))
        (br 0))
      (i64.const 0))
    (func (export "grow") (result i64)
      (if
        (i32.eq (memory.grow (i32.const 2)) (i32.const -1))
        (then (unreachable)))
      (i64.const 3))
    (func (export "extend") (result i64)
      (if
        (i32.eq (table.grow (ref.null func) (i32.const 4)) (i32.const -1))
        (then (unreachable)))
      (i64.const 5))
    (func (export "ping") (result i64)
      (i64.const 7)))
  (core instance $i (instantiate $m))
  (type $count (func (param "n" s64) (result s64)))
  (type $plain (func (result s64)))
  (func $burn (type $count) (canon lift (core func $i "burn")))
  (func $crawl (type $plain) (canon lift (core func $i "crawl")))
  (func $grow (type $plain) (canon lift (core func $i "grow")))
  (func $extend (type $plain) (canon lift (core func $i "extend")))
  (func $ping (type $plain) (canon lift (core func $i "ping")))
  (export "burn" (func $burn))
  (export "crawl" (func $crawl))
  (export "grow" (func $grow))
  (export "extend" (func $extend))
  (export "ping" (func $ping)))
