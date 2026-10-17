package Broken is function Value (N : Long_Long_Integer) return Long_Long_Integer with Export, Convention => C, External_Name => "broken_value"; end Broken;
