module matrix_math
  use iso_c_binding
contains
  function probe(a) result(value) bind(C, name="probe_column_major")
    real(c_double), intent(in) :: a(2,3)
    real(c_double) :: value
    value = 100.0_c_double * a(1,2) + a(2,1)
  end function probe
end module matrix_math
