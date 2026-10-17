package demo;

public final class Math {
    public static long add(long left, long right) {
        long total = left + right;
        return total;
    }

    public static void run() {
        System.out.println(add(2, 3));
    }

    public static long unsupported(long[] values) {
        return values.length;
    }
}
