with Ada.Text_IO; use Ada.Text_IO;
with Ada.Calendar;
with Interfaces; use Interfaces;
use type Ada.Calendar.Time;

procedure Ada_Bench is
   N      : constant Unsigned_64 := 400_000_000;
   Golden : constant Unsigned_64 := 16#9E3779B97F4A7C15#;
   M1     : constant Unsigned_64 := 16#BF58476D1CE4E5B9#;
   M2     : constant Unsigned_64 := 16#94D049BB133111EB#;
   X      : Unsigned_64 := Golden;
   W      : Unsigned_64 := 0;
   Hits   : Unsigned_64 := 0;
   Z, D   : Unsigned_64;
   T0     : constant Ada.Calendar.Time := Ada.Calendar.Clock;
   T1     : Ada.Calendar.Time;
   El     : Duration;
   Ms     : Float;
begin
   for I in 1 .. N loop
      X := X + Golden;
      Z := X;
      Z := (Z xor Shift_Right (Z, 30)) * M1;
      Z := (Z xor Shift_Right (Z, 27)) * M2;
      Z := Z xor Shift_Right (Z, 31);
      D := Z mod 10;
      W := (W mod 1_000_000_000) * 10 + D;
      if W = 1_718_103_620 then
         Hits := Hits + 1;
      end if;
   end loop;
   T1 := Ada.Calendar.Clock;
   El := T1 - T0;
   Ms := Float (N) / Float (El) / 1.0e6;
   Put_Line ("Ada: " & Float'Image (Ms) & " млн/сек | hits"
             & Unsigned_64'Image (Hits) & " | w" & Unsigned_64'Image (W));
end Ada_Bench;