using System;using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;

class Program
{
    static void Main()
    {
        const string target = "1718103620";

        long attempts = 0;
        long foundNumber = 0;

        int threadCount = Environment.ProcessorCount;

        using CancellationTokenSource cts = new();

        var stopwatch = Stopwatch.StartNew();

        Console.WriteLine("Поиск: " + target);
        Console.WriteLine("Количество потоков: " + threadCount);
        Console.WriteLine("Поиск запущен...\n");

        Task[] tasks = new Task[threadCount];

        for (int i = 0; i < threadCount; i++)
        {
            int threadId = i;

            tasks[i] = Task.Run(() =>
            {
                Random random = new Random(
                    Environment.TickCount + threadId * 1000
                );

                while (!cts.Token.IsCancellationRequested)
                {
                    long number = random.NextInt64(
                        100_000_000_000_000_000L,
                        9_000_000_000_000_000_000L
                    );

                    Interlocked.Increment(ref attempts);

                    if (number.ToString().Contains(target))
                    {
                        if (Interlocked.CompareExchange(
                            ref foundNumber, number, 0) == 0)
                        {
                            cts.Cancel();
                        }

                        break;
                    }
                }
            });
        }

        Task.WaitAll(tasks);

        stopwatch.Stop();

        Console.WriteLine("========== РЕЗУЛЬТАТ ==========");
        Console.WriteLine("Найденное число: " + foundNumber);
        Console.WriteLine("Всего генераций: " + attempts);
        Console.WriteLine("Время поиска: " + stopwatch.Elapsed);
        Console.WriteLine("Скорость: " +
            (attempts / stopwatch.Elapsed.TotalSeconds).ToString("N0") +
            " чисел/сек");
        Console.WriteLine("Потоков: " + threadCount);
    }
}