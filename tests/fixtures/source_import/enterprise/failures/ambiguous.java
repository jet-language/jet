package demo;
import other.Library;

public final class Ambiguous {
    public static long keep(long value) {
        return value;
    }

    public static long add(long value) {
        return value;
    }

    public static long add(long left, long right) {
        return left + right;
    }

    public static long quotient(long left, long right) {
        return left / right;
    }

    public static long broken(long value) {
        return value;
