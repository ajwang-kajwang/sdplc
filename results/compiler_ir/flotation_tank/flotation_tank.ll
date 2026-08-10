; ModuleID = 'flotation_tank'
source_filename = "flotation_tank"

define void @FlotationTank() {
entry:
  %cycle = alloca i32, align 4
  store i32 0, ptr %cycle, align 4
  %level = alloca float, align 4
  store float 5.000000e+01, ptr %level, align 4
  %air_flow = alloca float, align 4
  store float 3.000000e+01, ptr %air_flow, align 4
  %feed_flow = alloca float, align 4
  store float 4.000000e+01, ptr %feed_flow, align 4
  %tailings_flow = alloca float, align 4
  store float 3.800000e+01, ptr %tailings_flow, align 4
  %concentrate_grade = alloca float, align 4
  store float 8.200000e+01, ptr %concentrate_grade, align 4
  %target_level = alloca float, align 4
  store float 5.500000e+01, ptr %target_level, align 4
  %emergency_stop = alloca i1, align 1
  store i1 false, ptr %emergency_stop, align 1
  %motor_running = alloca i1, align 1
  store i1 true, ptr %motor_running, align 1
  %high_level = alloca i1, align 1
  store i1 false, ptr %high_level, align 1
  %low_air = alloca i1, align 1
  store i1 false, ptr %low_air, align 1
  %cycle1 = load i32, ptr %cycle, align 4
  %add = add i32 %cycle1, 1
  store i32 %add, ptr %cycle, align 4
  %level2 = load float, ptr %level, align 4
  %fext = fpext float %level2 to double
  %fgt = fcmp ogt double %fext, 7.000000e+01
  store i1 %fgt, ptr %high_level, align 1
  %air_flow3 = load float, ptr %air_flow, align 4
  %fext4 = fpext float %air_flow3 to double
  %flt = fcmp olt double %fext4, 2.000000e+01
  store i1 %flt, ptr %low_air, align 1
  %emergency_stop5 = load i1, ptr %emergency_stop, align 1
  br i1 %emergency_stop5, label %if.then, label %if.else

if.merge:                                         ; preds = %if.else, %if.then
  %motor_running11 = load i1, ptr %motor_running, align 1
  br i1 %motor_running11, label %if.then9, label %if.else10

if.then:                                          ; preds = %entry
  store i1 false, ptr %motor_running, align 1
  %air_flow6 = load float, ptr %air_flow, align 4
  %fext7 = fpext float %air_flow6 to double
  %fmul = fmul double %fext7, 0x3FEE666666666666
  %ftrunc = fptrunc double %fmul to float
  store float %ftrunc, ptr %air_flow, align 4
  br label %if.merge

if.else:                                          ; preds = %entry
  store i1 true, ptr %motor_running, align 1
  br label %if.merge

if.merge8:                                        ; preds = %if.else10, %if.merge26
  %high_level48 = load i1, ptr %high_level, align 1
  br i1 %high_level48, label %if.then46, label %if.else47

if.then9:                                         ; preds = %if.merge
  %level15 = load float, ptr %level, align 4
  %target_level16 = load float, ptr %target_level, align 4
  %fext17 = fpext float %level15 to double
  %fext18 = fpext float %target_level16 to double
  %flt19 = fcmp olt double %fext17, %fext18
  br i1 %flt19, label %if.then13, label %if.else14

if.else10:                                        ; preds = %if.merge
  %tailings_flow41 = load float, ptr %tailings_flow, align 4
  %fext42 = fpext float %tailings_flow41 to double
  %fmul43 = fmul double %fext42, 0x3FEF5C28F5C28F5C
  %ftrunc44 = fptrunc double %fmul43 to float
  store float %ftrunc44, ptr %tailings_flow, align 4
  br label %if.merge8

if.merge12:                                       ; preds = %if.else14, %if.then13
  %low_air29 = load i1, ptr %low_air, align 1
  br i1 %low_air29, label %if.then27, label %if.else28

if.then13:                                        ; preds = %if.then9
  %tailings_flow20 = load float, ptr %tailings_flow, align 4
  %fext21 = fpext float %tailings_flow20 to double
  %fsub = fsub double %fext21, 5.000000e-02
  %ftrunc22 = fptrunc double %fsub to float
  store float %ftrunc22, ptr %tailings_flow, align 4
  br label %if.merge12

if.else14:                                        ; preds = %if.then9
  %tailings_flow23 = load float, ptr %tailings_flow, align 4
  %fext24 = fpext float %tailings_flow23 to double
  %fadd = fadd double %fext24, 5.000000e-02
  %ftrunc25 = fptrunc double %fadd to float
  store float %ftrunc25, ptr %tailings_flow, align 4
  br label %if.merge12

if.merge26:                                       ; preds = %elsif.else.0, %elsif.then.0, %if.then27
  br label %if.merge8

if.then27:                                        ; preds = %if.merge12
  %air_flow30 = load float, ptr %air_flow, align 4
  %fext31 = fpext float %air_flow30 to double
  %fadd32 = fadd double %fext31, 2.500000e-01
  %ftrunc33 = fptrunc double %fadd32 to float
  store float %ftrunc33, ptr %air_flow, align 4
  br label %if.merge26

if.else28:                                        ; preds = %if.merge12
  %air_flow34 = load float, ptr %air_flow, align 4
  %fext35 = fpext float %air_flow34 to double
  %fgt36 = fcmp ogt double %fext35, 6.000000e+01
  br i1 %fgt36, label %elsif.then.0, label %elsif.else.0

elsif.then.0:                                     ; preds = %if.else28
  store float 6.000000e+01, ptr %air_flow, align 4
  br label %if.merge26

elsif.else.0:                                     ; preds = %if.else28
  %air_flow37 = load float, ptr %air_flow, align 4
  %fext38 = fpext float %air_flow37 to double
  %fadd39 = fadd double %fext38, 2.000000e-02
  %ftrunc40 = fptrunc double %fadd39 to float
  store float %ftrunc40, ptr %air_flow, align 4
  br label %if.merge26

if.merge45:                                       ; preds = %if.else47, %if.then46
  %level57 = load float, ptr %level, align 4
  %feed_flow58 = load float, ptr %feed_flow, align 4
  %tailings_flow59 = load float, ptr %tailings_flow, align 4
  %fext60 = fpext float %feed_flow58 to double
  %fext61 = fpext float %tailings_flow59 to double
  %fsub62 = fsub double %fext60, %fext61
  %fmul63 = fmul double %fsub62, 5.000000e-04
  %fext64 = fpext float %level57 to double
  %fadd65 = fadd double %fext64, %fmul63
  %ftrunc66 = fptrunc double %fadd65 to float
  store float %ftrunc66, ptr %level, align 4
  %air_flow67 = load float, ptr %air_flow, align 4
  %fext68 = fpext float %air_flow67 to double
  %fsub69 = fsub double %fext68, 2.500000e+01
  %fmul70 = fmul double %fsub69, 3.000000e-02
  %fadd71 = fadd double 8.200000e+01, %fmul70
  %ftrunc72 = fptrunc double %fadd71 to float
  store float %ftrunc72, ptr %concentrate_grade, align 4
  ret void

if.then46:                                        ; preds = %if.merge8
  %feed_flow49 = load float, ptr %feed_flow, align 4
  %fext50 = fpext float %feed_flow49 to double
  %fsub51 = fsub double %fext50, 1.000000e-01
  %ftrunc52 = fptrunc double %fsub51 to float
  store float %ftrunc52, ptr %feed_flow, align 4
  br label %if.merge45

if.else47:                                        ; preds = %if.merge8
  %feed_flow53 = load float, ptr %feed_flow, align 4
  %fext54 = fpext float %feed_flow53 to double
  %fadd55 = fadd double %fext54, 2.000000e-02
  %ftrunc56 = fptrunc double %fadd55 to float
  store float %ftrunc56, ptr %feed_flow, align 4
  br label %if.merge45
}
