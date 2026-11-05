#include <cstdio>
#include <cstdint>
#include <cstdlib>
#include <cuda_runtime.h>

#define P1 1718103620ULL
#define P2 518214ULL

static const uint64_t MOD1E9 = 1000000000ULL;

__device__ __forceinline__ uint64_t splitmix64(uint64_t& x)
{
    uint64_t z = (x += 0x9E3779B97F4A7C15ULL);
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
    return z ^ (z >> 31);
}

__global__ void hunt(uint64_t steps,
                     unsigned long long* digits,
                     unsigned long long* p1hits,
                     unsigned long long* p1b,
                     unsigned long long* p2hits,
                     unsigned long long* pairs,
                     unsigned long long* gapmin,
                     unsigned long long* found,
                     uint64_t gapmax)
{
    uint64_t tid = (uint64_t)blockIdx.x * blockDim.x + threadIdx.x;
    uint64_t x = tid * 0x9E3779B97F4A7C15ULL + 0x123456789ULL;
    uint64_t buf = 0;
    int have = 0;

    uint64_t w = 0;
    uint64_t local = 0;

    for (uint64_t i = 0; i < steps; ++i) {
        for (int half = 0; half < 2; ++half) {
            if (!have) {
                buf = splitmix64(x);
                have = 2;
            }
            uint32_t d = (uint32_t)(buf & 0xFFFFFFFFULL) % 10u;
            buf >>= 32;
            --have;

            w = (w % MOD1E9) * 10ULL + d;
            ++local;

            if ((w % 1000000ULL) == P2) atomicAdd(p2hits, 1ULL);
            if (w == 7181036201ULL) atomicAdd(p1b, 1ULL);

            if (w == P1) {
                atomicAdd(p1hits, 1ULL);
                uint32_t w2 = 0;
                uint64_t pend = gapmax + 6ULL;
                for (uint64_t k = 0; k < pend; ++k) {
                    if (!have) {
                        buf = splitmix64(x);
                        have = 2;
                    }
                    uint32_t d2 = (uint32_t)(buf & 0xFFFFFFFFULL) % 10u;
                    buf >>= 32;
                    --have;
                    w = (w % MOD1E9) * 10ULL + d2;
                    w2 = (w2 % 100000u) * 10u + d2;
                    ++local;
                    if (k >= 5ULL && w2 == (uint32_t)P2) {
                        uint64_t gap = k - 5ULL;
                        atomicAdd(pairs, 1ULL);
                        atomicMin(gapmin, gap);
                        atomicExch(found, (unsigned long long)tid);
                        break;
                    }
                }
            }
        }
    }
    atomicAdd(digits, (unsigned long long)local);
}

int main(int argc, char** argv)
{
    int threads = 256;
    int blocks = argc > 1 ? atoi(argv[1]) : 8192;
    uint64_t steps = argc > 2 ? strtoull(argv[2], nullptr, 10) : 2048ULL;
    int reps = argc > 3 ? atoi(argv[3]) : 30;
    uint64_t gapmax = argc > 4 ? strtoull(argv[4], nullptr, 10) : 5ULL;

    unsigned long long *d_digits, *d_p1, *d_p1b, *d_p2, *d_pairs, *d_gap, *d_found;
    cudaMalloc(&d_digits, 8); cudaMalloc(&d_p1, 8); cudaMalloc(&d_p1b, 8);
    cudaMalloc(&d_p2, 8); cudaMalloc(&d_pairs, 8); cudaMalloc(&d_gap, 8);
    cudaMalloc(&d_found, 8);
    cudaMemset(d_digits, 0, 8); cudaMemset(d_p1, 0, 8); cudaMemset(d_p1b, 0, 8);
    cudaMemset(d_p2, 0, 8); cudaMemset(d_pairs, 0, 8); cudaMemset(d_gap, 0xFF, 8);
    cudaMemset(d_found, 0, 8);

    printf("GPU splitmix64: blocks=%d threads=%d steps=%llu reps=%d gap=%llu\n",
           blocks, threads, (unsigned long long)steps, reps,
           (unsigned long long)gapmax);

    cudaEvent_t t0, t1;
    cudaEventCreate(&t0);
    cudaEventCreate(&t1);

    hunt<<<blocks, threads>>>(1000, d_digits, d_p1, d_p1b, d_p2, d_pairs, d_gap, d_found, gapmax);
    cudaDeviceSynchronize();

    cudaEventRecord(t0);
    for (int r = 0; r < reps; ++r)
        hunt<<<blocks, threads>>>(steps, d_digits, d_p1, d_p1b, d_p2, d_pairs, d_gap, d_found, gapmax);
    cudaEventRecord(t1);
    cudaEventSynchronize(t1);

    float ms = 0;
    cudaEventElapsedTime(&ms, t0, t1);

    unsigned long long h_digits = 0, h_p1 = 0, h_p1b = 0, h_p2 = 0;
    unsigned long long h_pairs = 0, h_gap = 0, h_found = 0;
    cudaMemcpy(&h_digits, d_digits, 8, cudaMemcpyDeviceToHost);
    cudaMemcpy(&h_p1, d_p1, 8, cudaMemcpyDeviceToHost);
    cudaMemcpy(&h_p1b, d_p1b, 8, cudaMemcpyDeviceToHost);
    cudaMemcpy(&h_p2, d_p2, 8, cudaMemcpyDeviceToHost);
    cudaMemcpy(&h_pairs, d_pairs, 8, cudaMemcpyDeviceToHost);
    cudaMemcpy(&h_gap, d_gap, 8, cudaMemcpyDeviceToHost);
    cudaMemcpy(&h_found, d_found, 8, cudaMemcpyDeviceToHost);

    double sec = ms / 1000.0;
    printf("цифр: %llu | время: %.2f сек | скорость: %.0f млн цифр/сек\n",
           h_digits, sec, (double)h_digits / sec / 1e6);
    printf("приветов: %llu (ожидание ~%.0f)\n", h_p1, (double)h_digits * 1e-10);
    printf("контроль: %llu (ожидание ~%.0f)\n", h_p1b, (double)h_digits * 1e-10);
    printf("другов: %llu (ожидание ~%.0f)\n", h_p2, (double)h_digits * 1e-6);
    printf("пар с зазором <= %llu: %llu", (unsigned long long)gapmax, h_pairs);
    if (h_pairs) printf(" | зазор: %llu | поток: %llu", h_gap, h_found);
    printf("\n");
    return 0;
}