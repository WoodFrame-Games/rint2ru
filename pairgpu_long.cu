#include <cstdio>
#include <cstdint>
#include <cstdlib>
#include <chrono>
#include <cuda_runtime.h>

#define P1 1718103620ULL
#define P2 518214ULL

static const uint64_t MOD1E9 = 1000000000ULL;
static const uint64_t GOLDEN = 0x9E3779B97F4A7C15ULL;

__device__ __forceinline__ uint64_t mix64(uint64_t z)
{
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
    return z ^ (z >> 31);
}

__device__ __forceinline__ uint64_t sm64(uint64_t& x)
{
    x += GOLDEN;
    uint64_t z = x;
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
    return z ^ (z >> 31);
}

__global__ void hunt(uint64_t steps, uint64_t batch,
                     unsigned long long* digits,
                     unsigned long long* p1hits,
                     unsigned long long* p2hits,
                     unsigned long long* pairs,
                     unsigned long long* gapmin,
                     uint64_t gapmax)
{
    uint64_t tid = (uint64_t)blockIdx.x * blockDim.x + threadIdx.x;
    uint64_t st = mix64(tid + batch * GOLDEN + 0x51ED270BULL);

    uint64_t w = 0;
    uint64_t local = 0;

    for (uint64_t i = 0; i < steps; ++i) {
        uint32_t d = (uint32_t)(sm64(st) % 10ULL);
        w = (w % MOD1E9) * 10ULL + d;
        ++local;

        if ((w % 1000000ULL) == P2) atomicAdd(p2hits, 1ULL);

        if (w == P1) {
            atomicAdd(p1hits, 1ULL);
            uint32_t w2 = 0;
            uint64_t pend = gapmax + 6ULL;
            for (uint64_t k = 0; k < pend; ++k) {
                uint32_t d2 = (uint32_t)(sm64(st) % 10ULL);
                w = (w % MOD1E9) * 10ULL + d2;
                w2 = (w2 % 100000u) * 10u + d2;
                ++local;
                if (k >= 5ULL && w2 == (uint32_t)P2) {
                    uint64_t gap = k - 5ULL;
                    atomicAdd(pairs, 1ULL);
                    atomicMin(gapmin, gap);
                    break;
                }
            }
        }
    }
    atomicAdd(digits, (unsigned long long)local);
}

int main(int argc, char** argv)
{
    int blocks = argc > 1 ? atoi(argv[1]) : 8192;
    int threads = argc > 2 ? atoi(argv[2]) : 256;
    uint64_t steps = argc > 3 ? strtoull(argv[3], nullptr, 10) : 4096ULL;
    int reps = argc > 4 ? atoi(argv[4]) : 8;
    uint64_t gapmax = argc > 5 ? strtoull(argv[5], nullptr, 10) : 5ULL;
    double duration = argc > 6 ? atof(argv[6]) : 3600.0;

    setvbuf(stdout, nullptr, _IONBF, 0);

    int dev = 0;
    cudaDeviceProp prop;
    cudaGetDeviceProperties(&prop, dev);
    printf("GPU long: %s | blocks=%d threads=%d steps=%llu reps=%d gap<=%llu duration=%.0f сек\n",
           prop.name, blocks, threads, (unsigned long long)steps, reps,
           (unsigned long long)gapmax, duration);

    unsigned long long *d_digits, *d_p1, *d_p2, *d_pairs, *d_gap;
    cudaMalloc(&d_digits, 8); cudaMalloc(&d_p1, 8); cudaMalloc(&d_p2, 8);
    cudaMalloc(&d_pairs, 8); cudaMalloc(&d_gap, 8);

    hunt<<<blocks, threads>>>(1000, 0, d_digits, d_p1, d_p2, d_pairs, d_gap, gapmax);
    cudaDeviceSynchronize();

    cudaMemset(d_digits, 0, 8); cudaMemset(d_p1, 0, 8); cudaMemset(d_p2, 0, 8);
    cudaMemset(d_pairs, 0, 8); cudaMemset(d_gap, 0xFF, 8);

    auto t0 = std::chrono::steady_clock::now();
    uint64_t batch = 1;
    double next_report = 10.0;

    for (;;) {
        for (int r = 0; r < reps; ++r)
            hunt<<<blocks, threads>>>(steps, batch++, d_digits, d_p1, d_p2, d_pairs, d_gap, gapmax);
        cudaError_t err = cudaDeviceSynchronize();
        if (err != cudaSuccess) {
            printf("CUDA error: %s\n", cudaGetErrorString(err));
            break;
        }

        double el = std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count();

        if (el >= next_report || el >= duration) {
            unsigned long long h_digits = 0, h_p1 = 0, h_p2 = 0, h_pairs = 0, h_gap = 0;
            cudaMemcpy(&h_digits, d_digits, 8, cudaMemcpyDeviceToHost);
            cudaMemcpy(&h_p1, d_p1, 8, cudaMemcpyDeviceToHost);
            cudaMemcpy(&h_p2, d_p2, 8, cudaMemcpyDeviceToHost);
            cudaMemcpy(&h_pairs, d_pairs, 8, cudaMemcpyDeviceToHost);
            cudaMemcpy(&h_gap, d_gap, 8, cudaMemcpyDeviceToHost);
            printf("... %6.0f сек | %8.3f трлн цифр | %4.0f млрд/сек | приветов: %llu (ожид ~%.0f) | пар: %llu | min gap: %llu\n",
                   el, (double)h_digits / 1e12, (double)h_digits / el / 1e9,
                   h_p1, (double)h_digits * 1e-10, h_pairs, h_gap);
            next_report += 10.0;
        }
        if (el >= duration) break;
    }

    double el = std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count();
    unsigned long long h_digits = 0, h_p1 = 0, h_p2 = 0, h_pairs = 0, h_gap = 0;
    cudaMemcpy(&h_digits, d_digits, 8, cudaMemcpyDeviceToHost);
    cudaMemcpy(&h_p1, d_p1, 8, cudaMemcpyDeviceToHost);
    cudaMemcpy(&h_p2, d_p2, 8, cudaMemcpyDeviceToHost);
    cudaMemcpy(&h_pairs, d_pairs, 8, cudaMemcpyDeviceToHost);
    cudaMemcpy(&h_gap, d_gap, 8, cudaMemcpyDeviceToHost);

    printf("\n========== РЕЗУЛЬТАТ (GPU T4, splitmix64) ==========\n");
    printf("цифр: %.3f трлн | время: %.0f сек | скорость: %.0f млрд цифр/сек\n",
           (double)h_digits / 1e12, el, (double)h_digits / el / 1e9);
    printf("приветов: %llu (ожидание ~%.0f)\n", h_p1, (double)h_digits * 1e-10);
    printf("контроль другов: %llu (ожидание ~%.0f)\n", h_p2, (double)h_digits * 1e-6);
    printf("пар с зазором <= %llu: %llu | min gap: %llu\n",
           (unsigned long long)gapmax, h_pairs, h_gap);
    return 0;
}