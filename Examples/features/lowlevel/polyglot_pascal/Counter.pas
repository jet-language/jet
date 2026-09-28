library counter;

type
  TCounter = class
  private
    FValue: Int64;
  public
    constructor Create(Value: Int64);
    function Add(Delta: Int64): Int64;
    destructor Destroy; override;
  end;

constructor TCounter.Create(Value: Int64);
begin
  inherited Create;
  FValue := Value;
end;

function TCounter.Add(Delta: Int64): Int64;
begin
  FValue := FValue + Delta;
  Result := FValue;
end;

destructor TCounter.Destroy;
begin
  inherited Destroy;
end;

function add_scalar(A, B: Int64): Int64; cdecl;
begin
  Result := A + B;
end;

function add_float(A, B: Double): Double; cdecl;
begin
  Result := A + B;
end;

function counter_new(Value: Int64): Pointer; cdecl;
begin
  Result := Pointer(TCounter.Create(Value));
end;

function counter_add(Handle: Pointer; Delta: Int64): Int64; cdecl;
begin
  Result := TCounter(Handle).Add(Delta);
end;

procedure counter_free(Handle: Pointer); cdecl;
begin
  TCounter(Handle).Free;
end;

exports add_scalar, add_float, counter_new, counter_add, counter_free;
begin
end.
