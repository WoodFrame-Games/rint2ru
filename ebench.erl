-module(ebench).
-export([main/0]).

main() ->
    N = 100000000,
    Mask = 16#FFFFFFFFFFFFFFFF,
    G = 16#9E3779B97F4A7C15,
    M1 = 16#BF58476D1CE4E5B9,
    M2 = 16#94D049BB133111EB,
    T0 = erlang:monotonic_time(microsecond),
    {W, Hits} = loop(N, G, 0, 0, G, M1, M2, Mask),
    T1 = erlang:monotonic_time(microsecond),
    El = (T1 - T0) / 1000000.0,
    Speed = N / El / 1000000.0,
    io:format("Erlang: ~.1f M digits/s | hits ~p | w ~p~n", [Speed, Hits, W]).

loop(0, _X, W, Hits, _G, _M1, _M2, _Mask) ->
    {W, Hits};
loop(I, X, W, Hits, G, M1, M2, Mask) ->
    X2 = (X + G) band Mask,
    Z0 = X2,
    Z1 = ((Z0 bxor (Z0 bsr 30)) * M1) band Mask,
    Z2 = ((Z1 bxor (Z1 bsr 27)) * M2) band Mask,
    Z3 = Z2 bxor (Z2 bsr 31),
    D = Z3 rem 10,
    W2 = ((W rem 1000000000) * 10 + D) band Mask,
    H2 = case W2 == 1718103620 of
             true -> Hits + 1;
             false -> Hits
         end,
    loop(I - 1, X2, W2, H2, G, M1, M2, Mask).