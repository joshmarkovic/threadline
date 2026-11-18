Unicode true
RequestExecutionLevel user
CRCCheck force
SetCompressor /SOLID lzma
ShowInstDetails show
ShowUninstDetails show

!include LogicLib.nsh
!include WinVer.nsh
!include x64.nsh
!include FileFunc.nsh

!ifndef LOCAL_FIXTURE
  !error "A reviewed publisher pin and signed package release flow are required before enabling a production installer."
!endif
!define Product "Threadline Installer Test"
!define Registration "Software\Microsoft\Windows\CurrentVersion\Uninstall\Threadline.InstallTest"
!define History "Software\Threadline.InstallTest"
!define MutexName "Global\Threadline.InstallTest.367F090C-C2DE-4998-8429-BBA2FDF0BD0C"
!define Stage ".threadline-stage"
!define StageId "367F090C-C2DE-4998-8429-BBA2FDF0BD0C"

Name "${Product} ${PACKAGE_VERSION}"
OutFile "${INSTALLER_OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\${Product}"

Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Var LockHandle
Var ResultCode
Var PriorBuild
Var PendingBuild
Var PriorVersion
Var PriorLocation
Var PriorApp
Var PriorUninstaller
Var AppReplaced
Var UninstallerReplaced
Var PathToInspect
Var ParentPath
Var RollbackFailed

!macro MessageAndStop code explanation
  DetailPrint "${explanation}"
  MessageBox MB_OK|MB_ICONSTOP "${explanation}" /SD IDOK
  SetErrorLevel ${code}
  Quit
!macroend

!macro LockInstaller
  System::Call 'kernel32::CreateMutexW(p 0, i 0, w "${MutexName}") p.r0 ?e'
  Pop $1
  StrCpy $LockHandle $0
  ${If} $0 == 0
  ${OrIf} $1 == 183
    !insertmacro MessageAndStop 13 "Another Threadline Installer Test operation is running. Wait for it to finish, then rerun this installer."
  ${EndIf}
!macroend

!macro RefuseReparse entry
  System::Call 'kernel32::GetFileAttributesW(w "${entry}") i.r0'
  ${If} $0 != -1
    IntOp $0 $0 & 0x400
    ${If} $0 != 0
      !insertmacro MessageAndStop 17 "The installation path contains a linked file or folder. Restore the ordinary per-user installation path and retry."
    ${EndIf}
  ${EndIf}
!macroend

!macro CheckAncestorPaths start
  StrCpy $PathToInspect "${start}"
  ${Do}
    ${If} $PathToInspect == ""
      ${ExitDo}
    ${EndIf}
    !insertmacro RefuseReparse "$PathToInspect"
    ${GetParent} "$PathToInspect" $ParentPath
    ${If} $ParentPath == $PathToInspect
      ${ExitDo}
    ${EndIf}
    StrCpy $PathToInspect $ParentPath
  ${Loop}
!macroend

!macro CheckPaths
  !insertmacro CheckAncestorPaths "$INSTDIR"
  !insertmacro CheckAncestorPaths "$SMPROGRAMS"
  !insertmacro RefuseReparse "$INSTDIR\threadline.exe"
  !insertmacro RefuseReparse "$INSTDIR\Uninstall.exe"
  !insertmacro RefuseReparse "$INSTDIR\${Stage}"
  !insertmacro RefuseReparse "$INSTDIR\${Stage}\owner.ini"
  !insertmacro RefuseReparse "$INSTDIR\${Stage}\candidate.exe"
  !insertmacro RefuseReparse "$INSTDIR\${Stage}\uninstaller.exe"
  !insertmacro RefuseReparse "$INSTDIR\${Stage}\previous.exe"
  !insertmacro RefuseReparse "$INSTDIR\${Stage}\previous-uninstaller.exe"
  !insertmacro RefuseReparse "$INSTDIR\${Stage}\removing.exe"
  !insertmacro RefuseReparse "$SMPROGRAMS\${Product}.lnk"
  ReadINIStr $1 "$INSTDIR\${Stage}\owner.ini" "Stage" "Identity"
  ${If} $1 != "${StageId}"
    IfFileExists "$INSTDIR\${Stage}\candidate.exe" foreign_stage 0
    IfFileExists "$INSTDIR\${Stage}\uninstaller.exe" foreign_stage 0
    IfFileExists "$INSTDIR\${Stage}\previous.exe" foreign_stage 0
    IfFileExists "$INSTDIR\${Stage}\previous-uninstaller.exe" foreign_stage 0
    IfFileExists "$INSTDIR\${Stage}\removing.exe" foreign_stage 0
    IfFileExists "$INSTDIR\${Stage}\owner.ini" foreign_stage owned_stage_checked
    foreign_stage:
    !insertmacro MessageAndStop 17 "The staging path contains files without a matching setup marker. Existing files were preserved."
    owned_stage_checked:
  ${EndIf}
!macroend

!macro RefuseRunningApp
  System::Call 'kernel32::SetEnvironmentVariableW(w "THREADLINE_SETUP_INSTANCE", w "$INSTDIR\threadline.exe") i.r0'
  ${If} $0 == 0
    !insertmacro MessageAndStop 18 "Could not prepare the application check. Rerun setup and check permission to install applications."
  ${EndIf}
  nsExec::ExecToStack /TIMEOUT=15000 '"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -Command "if (Get-Process -Name threadline -ErrorAction SilentlyContinue | Where-Object { $$_.Path -eq $$env:THREADLINE_SETUP_INSTANCE }) { exit 42 }"'
  Pop $ResultCode
  Pop $0
  ${If} $ResultCode == 42
    !insertmacro MessageAndStop 14 "Threadline Installer Test is running. Quit it from the application and rerun."
  ${ElseIf} $ResultCode != 0
    !insertmacro MessageAndStop 18 "Could not check whether Threadline is running. Ensure Windows PowerShell is available, then rerun setup."
  ${EndIf}
!macroend

!macro RefuseLockedExecutable
  ${If} ${FileExists} "$INSTDIR\threadline.exe"
    # Request delete access so a process denying replacement is detected before staging.
    System::Call 'kernel32::CreateFileW(w "$INSTDIR\threadline.exe", i 65536, i 7, p 0, i 3, i 128, p 0) p.r0 ?e'
    Pop $1
    IntOp $2 $0 + 0
    ${If} $2 == -1
      ${If} $1 == 32
      ${OrIf} $1 == 33
        !insertmacro MessageAndStop 14 "Release the installed application's open file and rerun."
      ${EndIf}
      !insertmacro MessageAndStop 16 "Setup cannot replace the installed application's file. Check permissions and rerun."
    ${EndIf}
    System::Call 'kernel32::CloseHandle(p r0)'
  ${EndIf}
!macroend

Function .onInit
  ${IfNot} ${IsNativeAMD64}
  ${OrIfNot} ${AtLeastWin11}
    !insertmacro MessageAndStop 11 "Threadline Installer Test requires Windows 11 on x64 hardware."
  ${EndIf}
  SetShellVarContext current
  SetRegView 64
  StrCpy $INSTDIR "$LOCALAPPDATA\Programs\${Product}"
  !insertmacro LockInstaller
  !insertmacro CheckPaths
  ReadRegStr $PriorLocation HKCU "${Registration}" "InstallLocation"
  ${If} $PriorLocation != ""
  ${AndIf} $PriorLocation != $INSTDIR
    !insertmacro MessageAndStop 17 "The registered location differs from the supported per-user path. The existing installation was preserved."
  ${EndIf}
  ${If} $PriorLocation == ""
    IfFileExists "$INSTDIR\threadline.exe" 0 no_unregistered_binary
      !insertmacro MessageAndStop 17 "An executable already exists in the fixture folder without a matching installation record. Its files were preserved."
    no_unregistered_binary:
    IfFileExists "$INSTDIR\Uninstall.exe" 0 no_unregistered_uninstaller
      !insertmacro MessageAndStop 17 "An unregistered uninstaller already exists. Its files were preserved."
    no_unregistered_uninstaller:
    IfFileExists "$SMPROGRAMS\${Product}.lnk" 0 no_unregistered_shortcut
      !insertmacro MessageAndStop 17 "A Start menu entry with this name already exists. Its files were preserved."
    no_unregistered_shortcut:
  ${EndIf}
  StrCpy $PriorBuild 0
  StrCpy $PendingBuild 0
  ReadRegDWORD $PriorBuild HKCU "${Registration}" "InstalledBuild"
  ReadRegDWORD $PendingBuild HKCU "${Registration}" "PendingBuild"
  ReadRegStr $PriorVersion HKCU "${Registration}" "DisplayVersion"
  ReadRegDWORD $0 HKCU "${History}" "HighestBuild"
  ${If} $0 != ""
  ${AndIf} ${PACKAGE_BUILD} < $0
    !insertmacro MessageAndStop 12 "This profile has been used with a newer test package. Use the same or a newer installer to preserve compatibility."
  ${EndIf}
  ${If} $PriorBuild != ""
  ${AndIf} ${PACKAGE_BUILD} < $PriorBuild
    !insertmacro MessageAndStop 12 "An older installer cannot replace this version. Use the same or a newer installer."
  ${EndIf}
  ${If} $PendingBuild != ""
  ${AndIf} ${PACKAGE_BUILD} < $PendingBuild
    !insertmacro MessageAndStop 12 "A newer setup was interrupted. Use its installer again to recover the installation."
  ${EndIf}
  !insertmacro RefuseRunningApp
  !insertmacro RefuseLockedExecutable
FunctionEnd

Function VerifyCandidate
  System::Call 'kernel32::SetEnvironmentVariableW(w "THREADLINE_SETUP_PAYLOAD", w "$INSTDIR\${Stage}\candidate.exe") i.r0'
  ${If} $0 == 0
    !insertmacro MessageAndStop 18 "Could not prepare installation verification. Rerun setup."
  ${EndIf}
  nsExec::ExecToStack /TIMEOUT=60000 '"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -Command "$$ErrorActionPreference = $\'Stop$\'; if ((Get-FileHash -LiteralPath $$env:THREADLINE_SETUP_PAYLOAD -Algorithm SHA256).Hash -ine $\'${PAYLOAD_SHA256}$\') { exit 43 }"'
  Pop $ResultCode
  Pop $0
  ${If} $ResultCode == 43
    Delete "$INSTDIR\${Stage}\candidate.exe"
    !insertmacro MessageAndStop 15 "The staged executable did not match the expected SHA-256 digest. Use an intact copy of this installer."
  ${ElseIf} $ResultCode != 0
    Delete "$INSTDIR\${Stage}\candidate.exe"
    !insertmacro MessageAndStop 18 "Could not verify the staged executable. Check Windows PowerShell and local installation permissions."
  ${EndIf}
FunctionEnd

Section "Install Threadline" SEC_INSTALL
  SetOverwrite on
  StrCpy $PriorApp 0
  StrCpy $PriorUninstaller 0
  StrCpy $AppReplaced 0
  StrCpy $UninstallerReplaced 0
  ClearErrors
  SetOutPath "$INSTDIR\${Stage}"
  IfErrors stage_failure
  WriteINIStr "$INSTDIR\${Stage}\owner.ini" "Stage" "Identity" "${StageId}"
  IfErrors stage_failure
  !ifdef FIXTURE_FAIL_STAGING
    Goto stage_failure
  !endif
  File /oname=candidate.exe "${PAYLOAD_FILE}"
  IfErrors stage_failure
  Call VerifyCandidate
  ClearErrors
  WriteUninstaller "$INSTDIR\${Stage}\uninstaller.exe"
  IfErrors stage_failure

  IfFileExists "$INSTDIR\threadline.exe" 0 prepare_uninstaller
  System::Call 'kernel32::CopyFileW(w "$INSTDIR\threadline.exe", w "$INSTDIR\${Stage}\previous.exe", i 0) i.r0'
  StrCmp $0 0 stage_failure
  StrCpy $PriorApp 1
  prepare_uninstaller:
  IfFileExists "$INSTDIR\Uninstall.exe" 0 record_pending
  System::Call 'kernel32::CopyFileW(w "$INSTDIR\Uninstall.exe", w "$INSTDIR\${Stage}\previous-uninstaller.exe", i 0) i.r0'
  StrCmp $0 0 stage_failure
  StrCpy $PriorUninstaller 1

  record_pending:
  ClearErrors
  WriteRegStr HKCU "${Registration}" "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKCU "${Registration}" "PendingBuild" ${PACKAGE_BUILD}
  IfErrors commit_failure
  SetOutPath "$INSTDIR"
  CreateShortCut "$SMPROGRAMS\${Product}.lnk" "$INSTDIR\threadline.exe"
  IfErrors commit_failure
  System::Call 'kernel32::MoveFileExW(w "$INSTDIR\${Stage}\uninstaller.exe", w "$INSTDIR\Uninstall.exe", i 9) i.r0'
  StrCmp $0 0 commit_failure
  StrCpy $UninstallerReplaced 1
  System::Call 'kernel32::MoveFileExW(w "$INSTDIR\${Stage}\candidate.exe", w "$INSTDIR\threadline.exe", i 9) i.r0'
  StrCmp $0 0 busy_failure
  StrCpy $AppReplaced 1
  ClearErrors
  WriteRegStr HKCU "${Registration}" "DisplayName" "${Product}"
  WriteRegStr HKCU "${Registration}" "DisplayVersion" "${PACKAGE_VERSION}"
  WriteRegStr HKCU "${Registration}" "DisplayIcon" "$INSTDIR\threadline.exe,0"
  WriteRegStr HKCU "${Registration}" "Publisher" "Threadline local test"
  WriteRegStr HKCU "${Registration}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${Registration}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${Registration}" "NoModify" 1
  WriteRegDWORD HKCU "${Registration}" "NoRepair" 1
  IfErrors commit_failure
  WriteRegDWORD HKCU "${Registration}" "InstalledBuild" ${PACKAGE_BUILD}
  IfErrors commit_failure
  WriteRegDWORD HKCU "${History}" "HighestBuild" ${PACKAGE_BUILD}
  IfErrors commit_failure
  DeleteRegValue HKCU "${Registration}" "PendingBuild"
  Delete "$INSTDIR\${Stage}\previous.exe"
  Delete "$INSTDIR\${Stage}\previous-uninstaller.exe"
  Delete "$INSTDIR\${Stage}\owner.ini"
  RMDir "$INSTDIR\${Stage}"
  DetailPrint "Installed ${Product} ${PACKAGE_VERSION} to $INSTDIR."
  DetailPrint "Launch ${Product} from the Start menu."
  SetErrorLevel 0
  Goto finished

  stage_failure:
  !insertmacro MessageAndStop 16 "Setup could not stage the complete update. Free disk space or check permissions and rerun."

  busy_failure:
  Call Rollback
  !insertmacro MessageAndStop 14 "Setup could not replace the installed executable. Quit Threadline, release open files and rerun."

  commit_failure:
  Call Rollback
  !insertmacro MessageAndStop 16 "Setup could not finish replacing files or registration. Check free space and permissions, then rerun the same installer."

  finished:
SectionEnd

Function Rollback
  StrCpy $RollbackFailed 0
  ${If} $AppReplaced == 1
    ${If} $PriorApp == 1
      System::Call 'kernel32::MoveFileExW(w "$INSTDIR\${Stage}\previous.exe", w "$INSTDIR\threadline.exe", i 9) i.r0'
      ${If} $0 == 0
        StrCpy $RollbackFailed 1
      ${EndIf}
    ${Else}
      ClearErrors
      Delete "$INSTDIR\threadline.exe"
      ${If} ${Errors}
        StrCpy $RollbackFailed 1
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ${If} $UninstallerReplaced == 1
    ${If} $PriorUninstaller == 1
      System::Call 'kernel32::MoveFileExW(w "$INSTDIR\${Stage}\previous-uninstaller.exe", w "$INSTDIR\Uninstall.exe", i 9) i.r0'
      ${If} $0 == 0
        StrCpy $RollbackFailed 1
      ${EndIf}
    ${Else}
      ClearErrors
      Delete "$INSTDIR\Uninstall.exe"
      ${If} ${Errors}
        StrCpy $RollbackFailed 1
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ${If} $RollbackFailed == 1
    !insertmacro MessageAndStop 19 "A file prevented rolling back the complete update. Setup kept its replacement files. Quit the application and rerun this exact installer to recover."
  ${EndIf}
  ClearErrors
  ${If} $PriorLocation == ""
    IfFileExists "$SMPROGRAMS\${Product}.lnk" 0 +2
      Delete "$SMPROGRAMS\${Product}.lnk"
    DeleteRegKey HKCU "${Registration}"
  ${Else}
    WriteRegStr HKCU "${Registration}" "DisplayVersion" "$PriorVersion"
    ${If} $PriorBuild == ""
      DeleteRegValue HKCU "${Registration}" "InstalledBuild"
    ${Else}
      WriteRegDWORD HKCU "${Registration}" "InstalledBuild" $PriorBuild
    ${EndIf}
    ${If} $PendingBuild == ""
      DeleteRegValue HKCU "${Registration}" "PendingBuild"
    ${Else}
      WriteRegDWORD HKCU "${Registration}" "PendingBuild" $PendingBuild
    ${EndIf}
  ${EndIf}
  ${If} ${Errors}
    !insertmacro MessageAndStop 19 "Setup could not restore the previous registration. Rerun this exact installer to recover."
  ${EndIf}
FunctionEnd

Function un.onInit
  SetShellVarContext current
  SetRegView 64
  ReadRegStr $0 HKCU "${Registration}" "InstallLocation"
  ${If} $INSTDIR != "$LOCALAPPDATA\Programs\${Product}"
  ${OrIf} $0 != $INSTDIR
    !insertmacro MessageAndStop 17 "The uninstaller does not match the registered per-user test installation. No files were removed."
  ${EndIf}
  !insertmacro LockInstaller
  !insertmacro CheckPaths
  !insertmacro RefuseRunningApp
  !insertmacro RefuseLockedExecutable
FunctionEnd

Section "Uninstall"
  ClearErrors
  SetOutPath "$INSTDIR\${Stage}"
  WriteINIStr "$INSTDIR\${Stage}\owner.ini" "Stage" "Identity" "${StageId}"
  IfErrors uninstall_failure
  IfFileExists "$INSTDIR\threadline.exe" 0 remove_shortcut
  System::Call 'kernel32::MoveFileExW(w "$INSTDIR\threadline.exe", w "$INSTDIR\${Stage}\removing.exe", i 9) i.r0'
  StrCmp $0 0 uninstall_busy

  remove_shortcut:
  ClearErrors
  Delete "$SMPROGRAMS\${Product}.lnk"
  IfErrors uninstall_restore
  ClearErrors
  DeleteRegKey HKCU "${Registration}"
  IfErrors uninstall_restore
  Delete "$INSTDIR\${Stage}\candidate.exe"
  Delete "$INSTDIR\${Stage}\uninstaller.exe"
  Delete "$INSTDIR\${Stage}\previous.exe"
  Delete "$INSTDIR\${Stage}\previous-uninstaller.exe"
  Delete "$INSTDIR\${Stage}\removing.exe"
  Delete "$INSTDIR\${Stage}\owner.ini"
  Delete "$INSTDIR\Uninstall.exe"
  SetOutPath "$LOCALAPPDATA\Programs"
  RMDir "$INSTDIR\${Stage}"
  RMDir "$INSTDIR"
  DetailPrint "Removed the application and Start menu entry. Local profile data and credentials are retained."
  SetErrorLevel 0
  Goto uninstall_finished

  uninstall_restore:
  IfFileExists "$INSTDIR\${Stage}\removing.exe" 0 uninstall_failure
  System::Call 'kernel32::MoveFileExW(w "$INSTDIR\${Stage}\removing.exe", w "$INSTDIR\threadline.exe", i 9) i.r0'
  StrCmp $0 0 uninstall_restoration_failure
  CreateShortCut "$SMPROGRAMS\${Product}.lnk" "$INSTDIR\threadline.exe"
  uninstall_failure:
  !insertmacro MessageAndStop 16 "The application files or Start menu entry could not be updated. Check the per-user installation permissions and retry uninstalling."

  uninstall_busy:
  !insertmacro MessageAndStop 14 "Quit Threadline and release open application files before uninstalling."

  uninstall_restoration_failure:
  !insertmacro MessageAndStop 19 "A file prevented restoring the executable. The uninstaller kept the application file in its staging folder. Quit the application and run the same setup to repair it."

  uninstall_finished:
SectionEnd
