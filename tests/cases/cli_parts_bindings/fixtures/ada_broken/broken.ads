with Interfaces.C;
package Broken is
   function Value (Input : Interfaces.C.long_long)
     return Interfaces.C.long_long
     with Export, Convention => C, External_Name => "broken_value";
end Broken;
