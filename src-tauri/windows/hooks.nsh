; VoxLoom's additions to Tauri's NSIS installer (see tauri.windows.conf.json).
;
; Uninstalling removes everything VoxLoom put on the computer. Tauri's own
; uninstaller removes the files it installed, the shortcuts and the
; uninstall entry. These hooks add the rest: whatever the app wrote into its
; folder later, the remembered install location, the downloaded models, the
; cache, the window's browser data and Windows' record of microphone use.
; The user's recordings and library (transcripts, notes, settings) go only
; when the box on the confirm page is ticked, or with uninstall.exe /S
; /DELETEDATA. Upgrading runs the old uninstaller first; then only the old
; program files go.

Var VoxLoomOwnFolder

!macro NSIS_HOOK_PREUNINSTALL
  ; A newer installer runs this uninstaller in place (_?=) before installing,
  ; by default ("Uninstall before installing") and without /UPDATE. Every
  ; real uninstall runs from a copy in a temporary folder instead. Treat the
  ; in-place run as the update it is, so upgrading keeps models, shortcuts
  ; and data even if the box was ticked.
  ${If} $EXEDIR == $INSTDIR
    StrCpy $UpdateMode 1
  ${EndIf}
  ${GetOptions} $CMDLINE "/DELETEDATA" $R0
  ${IfNot} ${Errors}
    StrCpy $DeleteAppDataCheckboxState 1
  ${EndIf}
  ; The folder is removed whole only when it is VoxLoom's own: named after
  ; the app and holding it. A user who installed into a shared folder keeps
  ; everything else in it.
  StrCpy $VoxLoomOwnFolder 0
  ${GetFileName} "$INSTDIR" $R0
  ${If} $R0 == "${PRODUCTNAME}"
  ${AndIf} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
  ${AndIf} ${FileExists} "$INSTDIR\worker\engine.py"
    StrCpy $VoxLoomOwnFolder 1
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    ${If} $VoxLoomOwnFolder = 1
      RMDir /r "$INSTDIR"
    ${EndIf}
    ; Tauri forgets the install location only with app data; forget it always.
    DeleteRegKey SHCTX "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty SHCTX "${MANUKEY}"
    DeleteRegValue HKCU "${MANUPRODUCTKEY}" "Installer Language"
    DeleteRegKey /ifempty HKCU "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty HKCU "${MANUKEY}"
    ; Windows lists a desktop app that used the microphone by its path, with
    ; "#" for "\".
    ${WordReplace} "$INSTDIR\${MAINBINARYNAME}.exe" "\" "#" "+" $R0
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged\$R0"
    ; The data of the user running the uninstaller. Models can be downloaded
    ; again; recordings cannot, so they stay unless the box was ticked.
    SetShellVarContext current
    RMDir /r "$LOCALAPPDATA\${BUNDLEID}\models"
    RMDir /r "$LOCALAPPDATA\${BUNDLEID}\cache"
    RMDir /r "$LOCALAPPDATA\${BUNDLEID}\EBWebView"
    RMDir /r "$APPDATA\${BUNDLEID}"
    RMDir "$LOCALAPPDATA\${BUNDLEID}"
  ${EndIf}
!macroend
