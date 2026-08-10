Var SQLiteCapsuleAssociationBeforeInstallWasOurs
Var SQLiteCapsuleAssociationBeforeInstallExisted
Var SQLiteCapsuleAssociationBackup
Var SQLiteCapsuleAssociationBackupExisted
Var SQLiteCapsuleAssociationAtUninstall
Var SQLiteCapsuleAssociationAtUninstallExisted
Var SQLiteCapsuleAssociationAtUninstallWasOurs
Var SQLiteCapsuleAssociationBackupWasPresent

!macro NSIS_HOOK_PREINSTALL
  StrCpy $SQLiteCapsuleAssociationBeforeInstallWasOurs 0
  StrCpy $SQLiteCapsuleAssociationBeforeInstallExisted 0
  StrCpy $SQLiteCapsuleAssociationBackupExisted 0

  ClearErrors
  ReadRegStr $R0 SHCTX "Software\Classes\.sqlitecapsule" ""
  ${IfNot} ${Errors}
    StrCpy $SQLiteCapsuleAssociationBeforeInstallExisted 1
  ${EndIf}
  ${If} $R0 == "SQLite Capsule"
    StrCpy $SQLiteCapsuleAssociationBeforeInstallWasOurs 1
    ClearErrors
    ReadRegStr $SQLiteCapsuleAssociationBackup SHCTX "Software\Classes\.sqlitecapsule" "SQLite Capsule_backup"
    ${IfNot} ${Errors}
      StrCpy $SQLiteCapsuleAssociationBackupExisted 1
    ${EndIf}
    ; Migrate installations produced before the explicit presence sentinel.
    ClearErrors
    ReadRegDWORD $R1 SHCTX "${MANUPRODUCTKEY}" "AssociationBackupWasPresent"
    ${If} ${Errors}
      ${If} $SQLiteCapsuleAssociationBackupExisted = 1
      ${AndIf} $SQLiteCapsuleAssociationBackup != ""
        WriteRegDWORD SHCTX "${MANUPRODUCTKEY}" "AssociationBackupWasPresent" 1
      ${Else}
        WriteRegDWORD SHCTX "${MANUPRODUCTKEY}" "AssociationBackupWasPresent" 0
      ${EndIf}
    ${EndIf}
  ${Else}
    ${If} $SQLiteCapsuleAssociationBeforeInstallExisted = 1
      WriteRegDWORD SHCTX "${MANUPRODUCTKEY}" "AssociationBackupWasPresent" 1
    ${Else}
      WriteRegDWORD SHCTX "${MANUPRODUCTKEY}" "AssociationBackupWasPresent" 0
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Tauri's association macro overwrites its backup during a reinstall. Preserve
  ; the original pre-host association so a later uninstall restores it.
  ${If} $SQLiteCapsuleAssociationBeforeInstallWasOurs = 1
    ${If} $SQLiteCapsuleAssociationBackupExisted = 1
      WriteRegStr SHCTX "Software\Classes\.sqlitecapsule" "SQLite Capsule_backup" "$SQLiteCapsuleAssociationBackup"
    ${Else}
      DeleteRegValue SHCTX "Software\Classes\.sqlitecapsule" "SQLite Capsule_backup"
    ${EndIf}
  ${EndIf}

  ; Tauri quotes the selected file but not the installed executable in its
  ; generated command. The per-user install path contains spaces, so replace it
  ; with a command that quotes both paths before notifying Explorer.
  WriteRegStr SHCTX "Software\Classes\SQLite Capsule\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
  !insertmacro UPDATEFILEASSOC

  ClearErrors
  CreateDirectory "$INSTDIR\installer-cache"
  IfErrors sqlite_capsule_cache_failed
  ClearErrors
  CopyFiles /SILENT "$EXEPATH" "$INSTDIR\installer-cache\sqlite-capsule-host-current.exe"
  IfErrors sqlite_capsule_cache_failed
  Goto sqlite_capsule_cache_done

  sqlite_capsule_cache_failed:
    Abort "SQLite Capsule Host could not retain its signed installer for verified rollback."

  sqlite_capsule_cache_done:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  StrCpy $SQLiteCapsuleAssociationAtUninstallExisted 0
  StrCpy $SQLiteCapsuleAssociationAtUninstallWasOurs 0
  StrCpy $SQLiteCapsuleAssociationBackupWasPresent 0

  ClearErrors
  ReadRegStr $SQLiteCapsuleAssociationAtUninstall SHCTX "Software\Classes\.sqlitecapsule" ""
  ${IfNot} ${Errors}
    StrCpy $SQLiteCapsuleAssociationAtUninstallExisted 1
  ${EndIf}
  ${If} $SQLiteCapsuleAssociationAtUninstall == "SQLite Capsule"
    StrCpy $SQLiteCapsuleAssociationAtUninstallWasOurs 1
  ${EndIf}
  ReadRegDWORD $SQLiteCapsuleAssociationBackupWasPresent SHCTX "${MANUPRODUCTKEY}" "AssociationBackupWasPresent"

  Delete "$INSTDIR\installer-cache\sqlite-capsule-host-current.exe"
  RMDir "$INSTDIR\installer-cache"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; APP_UNASSOCIATE restores the saved value but leaves its private backup and,
  ; when no association existed before installation, an empty extension key.
  ; It also overwrites an association the user may have chosen after install.
  DeleteRegValue SHCTX "Software\Classes\.sqlitecapsule" "SQLite Capsule_backup"
  ${If} $SQLiteCapsuleAssociationAtUninstallWasOurs = 1
    ${If} $SQLiteCapsuleAssociationBackupWasPresent <> 1
      DeleteRegValue SHCTX "Software\Classes\.sqlitecapsule" ""
    ${EndIf}
  ${Else}
    ${If} $SQLiteCapsuleAssociationAtUninstallExisted = 1
      WriteRegStr SHCTX "Software\Classes\.sqlitecapsule" "" "$SQLiteCapsuleAssociationAtUninstall"
    ${Else}
      DeleteRegValue SHCTX "Software\Classes\.sqlitecapsule" ""
    ${EndIf}
  ${EndIf}
  DeleteRegKey /ifempty SHCTX "Software\Classes\.sqlitecapsule"
  !insertmacro UPDATEFILEASSOC

  ; The generated uninstaller only removes this installer bookkeeping when the
  ; user also elects to delete application data. It is not application data and
  ; must not survive an ordinary uninstall.
  DeleteRegKey SHCTX "${MANUPRODUCTKEY}"
  DeleteRegKey /ifempty SHCTX "${MANUKEY}"
!macroend
