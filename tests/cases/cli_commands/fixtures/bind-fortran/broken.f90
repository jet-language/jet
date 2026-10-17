module broken_math
  use iso_c_binding
contains
  function broken(a) result(value) bind(C, name="broken")
    integer(c_int64_t), value :: a
    integer(c_int64_t) :: value
    value = a +
  end function broken
end module broken_math
