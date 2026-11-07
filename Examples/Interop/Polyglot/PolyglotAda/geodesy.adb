with Interfaces.C;
use type Interfaces.C.double;
use type Interfaces.C.long_long;
package body Geodesy is
   Calls_Count : Interfaces.C.long_long := 0;
   function Double_Lat (Lat : Latitude) return Interfaces.C.double is
   begin
      Calls_Count := Calls_Count + 1;
      return Lat * 2.0;
   end Double_Lat;
   function Calls (Unused : Interfaces.C.long_long) return Interfaces.C.long_long is
   begin
      return Calls_Count + Unused;
   end Calls;
end Geodesy;
