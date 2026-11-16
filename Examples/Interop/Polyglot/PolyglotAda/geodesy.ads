with Interfaces.C;
use type Interfaces.C.double;
package Geodesy is
   subtype Latitude is Interfaces.C.double range -90.0 .. 90.0;
   function Double_Lat (Lat : Latitude) return Interfaces.C.double
     with Export, Convention => C, External_Name => "geo_double";
   function Calls (Unused : Interfaces.C.long_long) return Interfaces.C.long_long
     with Export, Convention => C, External_Name => "geo_calls";
end Geodesy;
