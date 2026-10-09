using System;

namespace Demo {
    public static class Math {
        public static long Add(long left, long right) {
            long total = left + right;
            return total;
        }

        public static void run() {
            Console.WriteLine(Add(2, 3));
        }

        public static long Unsupported(long[] values) {
            return values.Length;
        }
    }
}
