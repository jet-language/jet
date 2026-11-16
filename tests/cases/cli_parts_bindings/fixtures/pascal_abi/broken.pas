library broken;
type TCounter = class end;
function counter_new(Value: Int64): Pointer; cdecl; begin Result := nil; end;
function counter_bad(Handle: Pointer; Value: Integer): Int64; cdecl; begin Result := 0; end;
procedure counter_free(Handle: Pointer); cdecl; begin end;
exports counter_new, counter_bad, counter_free;
begin end.
