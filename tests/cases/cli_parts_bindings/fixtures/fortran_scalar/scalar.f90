module scalar_math
  use iso_c_binding
contains
  function add_i64(a, b) result(value) bind(C, name="add_i64")
    integer(c_int64_t), value :: a
    integer(c_int64_t), value :: b
    integer(c_int64_t) :: value
    value = a + b
  end function add_i64
end module scalar_math
