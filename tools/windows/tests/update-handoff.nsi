Unicode true
RequestExecutionLevel user
SilentInstall silent
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
FunctionEnd
Section
  WriteINIStr "$EXEDIR\result.ini" "result" "restart" "$UpdateRestart"
  WriteINIStr "$EXEDIR\result.ini" "result" "code" "0"
SectionEnd
