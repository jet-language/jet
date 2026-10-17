with Interfaces.C;
package Geodesy is
   subtype Latitude is Interfaces.C.double range -90.0 .. 90.0;
   function Double_Lat (Lat : Latitude) return Interfaces.C.double
     with Export, Convention => C, External_Name => "geo_double";
end Geodesy;
