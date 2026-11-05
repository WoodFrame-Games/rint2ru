#include <cstdio>
#include <cstdint>
#include <cuda_runtime.h>

__device__ __forceinline__ uint64_t splitmix64(uint64_t& x)
{
    uint64_t z = (x += 0x9E3779B97F4A7C15ULL);
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
    return z ^ (z >> 31);
}

__global__ void hist_kernel(unsigned long long steps, unsigned long long* hist)
{
    uint64_t tid = (uint64_t)blockIdx.x * blockDim.x + threadIdx.x;
    uint64_t x = tid * 0x9E3779B97F4A7C15ULL + 0x123456789ULL;
    unsigned long long h[10] = {0};
    uint64_t buf = 0;
    int have = 0;
    for (unsigned long long i = 0; i < steps; ++i) {
        if (!have) { buf = splitmix64(x); have = 2; }
        uint32_t d = (uint32_t)(buf & 0xFFFFFFFFULL) % 10u;
        buf >>= 32;
        --have;
        h[d]++;
    }
    for (int d = 0; d < 10; ++d) atomicAdd(&hist[d], h[d]);
}

int main()
{
    int blocks = 8192, threads = 256;
    unsigned long long steps = 2048;
    unsigned long long* d_hist;
    unsigned long long h[10] = {0};
    cudaMalloc(&d_hist, sizeof(h));
    cudaMemset(d_hist, 0, sizeof(h));
    hist_kernel<<<blocks, threads>>>(steps, d_hist);
    cudaDeviceSynchronize();
    cudaMemcpy(h, d_hist, sizeof(h), cudaMemcpyDeviceToHost);
    unsigned long long total = 0;
    for (int d = 0; d < 10; ++d) total += h[d];
    printf("GPU digits: %llu\n  histogram:", total);
    for (int d = 0; d < 10; ++d)
        printf(" %d:%.3f%%", d, 100.0 * (double)h[d] / (double)total);
    printf("\n");
    return 0;
}