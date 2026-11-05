function main()
    n = 400_000_000
    x = UInt64(0x9E3779B97F4A7C15)
    w = UInt64(0)
    hits = 0
    t0 = time()
    @inbounds for i in 1:n
        x += 0x9E3779B97F4A7C15
        z = x
        z = (z ⊻ (z >> 30)) * 0xBF58476D1CE4E5B9
        z = (z ⊻ (z >> 27)) * 0x94D049BB133111EB
        z = z ⊻ (z >> 31)
        d = z % 10
        w = (w % 1_000_000_000) * 10 + d
        if w == 1_718_103_620
            hits += 1
        end
    end
    el = time() - t0
    println("Julia: ", round(Int, n / el / 1e6), " млн цифр/сек hits ", hits, " w ", w)
end

main()