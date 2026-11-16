with Interfaces.C;
package body Broken is
   function Value (Input : Interfaces.C.long_long) return Interfaces.C.long_long is
   begin
      return Input + ;
   end Value;
end Broken;
