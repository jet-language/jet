package body Broken is function Value (N : Long_Long_Integer) return Long_Long_Integer is begin return N +; end Value; end Broken;
