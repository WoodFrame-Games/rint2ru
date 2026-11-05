program fbench
  implicit none
  integer(8) :: n, i, x, z, w, d, hits

  n = 400000000_8
  x = -7046029254386353131_8
  w = 0_8
  hits = 0_8

  do i = 1_8, n
    x = x + (-7046029254386353131_8)
    z = x
    z = ieor(z, rshift(z, 30)) * (-4658895280553007687_8)
    z = ieor(z, rshift(z, 27)) * (-7723592293110705685_8)
    z = ieor(z, rshift(z, 31))
    d = modulo(z, 10_8)
    w = modulo(w, 1000000000_8) * 10_8 + d
    if (w == 1718103620_8) hits = hits + 1_8
  end do

  write (*, '(A,I0,A,I0)') 'Fortran: hits=', hits, ' w=', w
end program fbench