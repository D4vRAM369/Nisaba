; Añade la carpeta de instalación al Path del usuario para que `nisaba` funcione en cualquier terminal.
; ponytail: PowerShell en vez de manipular cadenas en NSIS; una ruta con comilla simple (O'Brien) rompería el comando.
!macro NSIS_HOOK_POSTINSTALL
  nsExec::Exec `powershell -NoProfile -ExecutionPolicy Bypass -Command "$$d='$INSTDIR'; $$p=[Environment]::GetEnvironmentVariable('Path','User'); $$l=@(($$p -split ';') | Where-Object { $$_ -and $$_ -ne $$d }); [Environment]::SetEnvironmentVariable('Path', (($$l + $$d) -join ';'), 'User')"`
  Pop $0
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::Exec `powershell -NoProfile -ExecutionPolicy Bypass -Command "$$d='$INSTDIR'; $$p=[Environment]::GetEnvironmentVariable('Path','User'); $$l=@(($$p -split ';') | Where-Object { $$_ -and $$_ -ne $$d }); [Environment]::SetEnvironmentVariable('Path', ($$l -join ';'), 'User')"`
  Pop $0
!macroend
