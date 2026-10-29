; Añade la carpeta de instalación al Path del usuario para que `nisaba` funcione en cualquier terminal.
; Lee y escribe el valor sin expandir (REG_EXPAND_SZ) para no convertir %USERPROFILE%\… de otras entradas en rutas fijas.
; Borrar una variable temporal con .NET emite WM_SETTINGCHANGE, así las terminales nuevas ven el cambio.
; ponytail: PowerShell en vez de manipular cadenas en NSIS; una ruta con comilla simple (O'Brien) rompería el comando.
!macro NSIS_HOOK_POSTINSTALL
  nsExec::Exec `powershell -NoProfile -ExecutionPolicy Bypass -Command "$$d='$INSTDIR'; $$k='HKCU:\Environment'; $$p=(Get-Item $$k).GetValue('Path','','DoNotExpandEnvironmentNames'); $$l=@(($$p -split ';') | Where-Object { $$_ -and $$_ -ne $$d }); Set-ItemProperty $$k Path (($$l + $$d) -join ';') -Type ExpandString; [Environment]::SetEnvironmentVariable('NisabaAviso','1','User'); [Environment]::SetEnvironmentVariable('NisabaAviso',$$null,'User')"`
  Pop $0
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::Exec `powershell -NoProfile -ExecutionPolicy Bypass -Command "$$d='$INSTDIR'; $$k='HKCU:\Environment'; $$p=(Get-Item $$k).GetValue('Path','','DoNotExpandEnvironmentNames'); $$l=@(($$p -split ';') | Where-Object { $$_ -and $$_ -ne $$d }); Set-ItemProperty $$k Path ($$l -join ';') -Type ExpandString; [Environment]::SetEnvironmentVariable('NisabaAviso','1','User'); [Environment]::SetEnvironmentVariable('NisabaAviso',$$null,'User')"`
  Pop $0
!macroend
