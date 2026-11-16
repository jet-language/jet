library inventory;
function add_scalar(A, B: Int64): Int64; cdecl;
begin Result := A + B; end;
exports add_scalar;
begin end.
