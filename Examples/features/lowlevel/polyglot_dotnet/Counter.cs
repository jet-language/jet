public class Counter
{
    private long value;

    public Counter(long value)
    {
        this.value = value;
    }

    public long add(long amount)
    {
        value += amount;
        return value;
    }

    public long explode(long code)
    {
        if (code < 0)
        {
            throw new System.InvalidOperationException("hidden managed detail");
        }
        return code;
    }

    public static double twice(double value)
    {
        return value * 2.0;
    }
}
