PROGRAM FlotationTank
VAR
    cycle : DINT := 0;
    level : REAL := 50.0;
    air_flow : REAL := 30.0;
    feed_flow : REAL := 40.0;
    tailings_flow : REAL := 38.0;
    concentrate_grade : REAL := 82.0;
    target_level : REAL := 55.0;
    emergency_stop : BOOL := FALSE;
    motor_running : BOOL := TRUE;
    high_level : BOOL := FALSE;
    low_air : BOOL := FALSE;
END_VAR

cycle := cycle + 1;

high_level := level > 70.0;
low_air := air_flow < 20.0;

IF emergency_stop THEN
    motor_running := FALSE;
    air_flow := air_flow * 0.95;
ELSE
    motor_running := TRUE;
END_IF;

IF motor_running THEN
    IF level < target_level THEN
        tailings_flow := tailings_flow - 0.05;
    ELSE
        tailings_flow := tailings_flow + 0.05;
    END_IF;

    IF low_air THEN
        air_flow := air_flow + 0.25;
    ELSIF air_flow > 60.0 THEN
        air_flow := 60.0;
    ELSE
        air_flow := air_flow + 0.02;
    END_IF;
ELSE
    tailings_flow := tailings_flow * 0.98;
END_IF;

IF high_level THEN
    feed_flow := feed_flow - 0.10;
ELSE
    feed_flow := feed_flow + 0.02;
END_IF;

level := level + (feed_flow - tailings_flow) * 0.0005;
concentrate_grade := 82.0 + (air_flow - 25.0) * 0.03;

END_PROGRAM
