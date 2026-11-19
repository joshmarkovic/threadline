Unicode true
RequestExecutionLevel user
Page instfiles
!include FileFunc.nsh
!define PACKAGE_BUILD 2
!define THREADLINE_HANDOFF_TIMEOUT 2000
Name "Threadline temporary update handoff test"
OutFile "${HARNESS_OUTPUT}"
Var PriorLocation
Var PriorBuild
Var UpdateRestart
!macro MessageAndStop code explanation
  WriteINIStr "$EXEDIR\result.ini" "result" "code" "${code}"
  SetErrorLevel ${code}
  Quit
!macroend
!include "..\update-handoff.nsh"
Function .onInit
  ${GetParent} "$EXEDIR" $INSTDIR
  StrCpy $PriorLocation "$INSTDIR"
  StrCpy $PriorBuild 1
  Call WaitForUpdateParent
  ${GetParameters} $0
  StrCmp $0 "/fixture-fail" 0 startup_ready
    !insertmacro MessageAndStop 16 "Synthetic setup failure"
  startup_ready:
FunctionEnd
Section
  Call FinishSuccessfulInstall
  WriteINIStr "$EXEDIR\result.ini" "result" "restart" "$UpdateRestart"
  WriteINIStr "$EXEDIR\result.ini" "result" "code" "0"
SectionEnd
