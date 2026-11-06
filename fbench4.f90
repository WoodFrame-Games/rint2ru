program fbench
  implicit none
  integer(8) :: n, i, x, z, w, d, hits, c0, c1, rate
  real(8) :: el, speed

  n = 400000000_8
  x = -7046029254386353131_8
  w = 0_8
  hits = 0_8

  call system_clock(c0, rate)
  do i = 1_8, n
    x = x + (-7046029254386353131_8)
    z = x
    z = ieor(z, ishft(z, -30)) * (-4658895280553007687_8)
    z = ieor(z, ishft(z, -27)) * (-7723592293110705685_8)
    z = ieor(z, ishft(z, -31))
    d = modulo(z, 10_8)
    if (z < 0_8) d = modulo(d + 6_8, 10_8)
    w = modulo(w, 1000000000_8) * 10_8 + d
    if (w == 1718103620_8) hits = hits + 1_8
  end do
  call system_clock(c1)

  el = real(c1 - c0, 8) / real(rate, 8)
  speed = real(n, 8) / el / 1.0e6
  write (*, '(A,F8.0,A,I0,A,I0)') 'Fortran: ', speed, ' млн/сек | hits ', hits, ' | w ', w
end program fbench