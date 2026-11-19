Function WaitForUpdateParent
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} "$R0" "/THREADLINEUPDATE=" $R1
  IfErrors update_manual
  # A handoff always upgrades an established registration and binds one exact process handle.
  StrCmp $PriorLocation "$INSTDIR" 0 update_refused
  IntCmp ${PACKAGE_BUILD} $PriorBuild update_refused update_refused 0
  StrCpy $0 $R1
  IntFmt $1 "%u" $0
  StrCmpS $1 $R1 0 update_refused
  StrCmp $1 "0" update_refused
  System::Call 'kernel32::OpenProcess(i 0x101000, i 0, i r0) p.r2'
  StrCmp $2 0 update_refused
  System::Call 'kernel32::QueryFullProcessImageNameW(p r2, i 0, w .r3, *i ${NSIS_MAX_STRLEN}) i.r0'
  StrCmp $0 0 update_refused
  StrCmp $3 "$INSTDIR\threadline.exe" 0 update_refused
  ${GetFileName} "$EXEDIR" $R0
  StrCpy $R0 "Local\Threadline.Update.$R1.$R0"
  System::Call 'kernel32::OpenEventW(i 2, i 0, w "$R0.Ready") p.r3'
  StrCmp $3 0 update_refused
  System::Call 'kernel32::OpenEventW(i 0x100000, i 0, w "$R0.Proceed") p.r4'
  StrCmp $4 0 update_refused
  System::Call 'kernel32::SetEvent(p r3) i.r0'
  StrCmp $0 0 update_refused
  System::Call 'kernel32::WaitForSingleObject(p r4, i ${THREADLINE_HANDOFF_TIMEOUT}) i.r0'
  StrCmp $0 0 0 update_refused
  System::Call 'kernel32::WaitForSingleObject(p r2, i ${THREADLINE_HANDOFF_TIMEOUT}) i.r0'
  StrCmp $0 0 0 update_refused
  System::Call 'kernel32::CloseHandle(p r2)'
  System::Call 'kernel32::CloseHandle(p r3)'
  System::Call 'kernel32::CloseHandle(p r4)'
  StrCpy $UpdateRestart 1
  SetAutoClose true
  Return
  update_refused:
    !insertmacro MessageAndStop 21 "Threadline could not confirm the update and close in time. Open Threadline and try the update again."
  update_manual:
    ClearErrors
FunctionEnd
