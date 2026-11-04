// Normalise uniquement l’adresse. Les identifiants restent dans leurs champs dédiés.
export function normalizeSftpAddress(value, fieldPort) {
  if (!Number.isInteger(fieldPort) || fieldPort < 1 || fieldPort > 65535) throw new Error('Indiquez un port SFTP entre 1 et 65535.');
  let address = value.trim();
  if (!address) throw new Error('Renseignez le nom du serveur SFTP.');
  if (address.includes('\\')) throw new Error('Adresse SFTP invalide : utilisez le nom du serveur, par exemple sftp.exemple.fr.');
  if (!address.includes('://')) {
    if ((address.match(/:/g) || []).length > 1 && /^[0-9a-f:.]+$/i.test(address)) address = `[${address}]`;
    address = 'sftp://' + address;
  }
  let parsed;
  try { parsed = new URL(address); } catch { throw new Error('Adresse SFTP invalide : utilisez le nom du serveur ou une adresse sftp://serveur:port.'); }
  if (parsed.protocol !== 'sftp:') throw new Error('Utilisez une adresse SFTP. Les adresses FTP, FTPS et HTTPS ne désignent pas une connexion SFTP.');
  if (parsed.username || parsed.password) throw new Error('Saisissez l’identifiant et le mot de passe dans leurs champs dédiés, pas dans l’adresse du serveur.');
  if ((parsed.pathname && parsed.pathname !== '/') || parsed.search || parsed.hash) throw new Error('Indiquez le dossier dans « Dossier distant existant » et gardez seulement le serveur dans son champ.');
  const explicitPort = parsed.port ? Number(parsed.port) : null;
  if (explicitPort !== null && fieldPort !== 22 && fieldPort !== explicitPort) throw new Error('Le port de l’adresse et le champ « Port SFTP » sont différents. Indiquez le même port dans les deux.');
  const host = parsed.hostname.replace(/^\[|\]$/g, '').toLowerCase();
  const port = explicitPort ?? fieldPort;
  if (!host || host.includes('%') || /\s/.test(host) || port < 1 || port > 65535) throw new Error('Adresse ou port SFTP invalide.');
  return {host, port};
}
