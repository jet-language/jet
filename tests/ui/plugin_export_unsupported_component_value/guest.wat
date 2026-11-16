;; One export whose parameter is a Component record, which has no Jet spelling.
(component
  (core module $m
    (func (export "score") (param i64) (result i64) (local.get 0)))
  (core instance $i (instantiate $m))
  (type $score-type (func (param "row" (record (field "score" s64))) (result s64)))
  (func $score (type $score-type) (canon lift (core func $i "score")))
  (export "score" (func $score)))
