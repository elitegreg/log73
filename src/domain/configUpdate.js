export function buildConfigUpdatePayload({
  loginUser,
  loginPassword,
  loginPasswordConfirm,
  disableLogin,
  dxClusterEnabled,
  dxClusterHost,
  dxClusterPort,
  dxClusterCallsign,
  dxClusterMaxAgeMin,
  dxClusterCommands,
}) {
  const payload = {
    login_user: loginUser,
    disable_login: Boolean(disableLogin),
    dxcluster_enabled: dxClusterEnabled,
    dxcluster_host: dxClusterHost,
    dxcluster_port: Number.parseInt(dxClusterPort, 10) || 23,
    dxcluster_callsign: dxClusterCallsign,
    dxcluster_max_age_min: Number.parseInt(dxClusterMaxAgeMin, 10) || 60,
    dxcluster_commands: dxClusterCommands,
  };

  if (
    !payload.disable_login &&
    (loginPassword !== '' || loginPasswordConfirm !== '')
  ) {
    payload.login_password_change = loginPassword;
    payload.login_password_confirm = loginPasswordConfirm;
  }

  return payload;
}
