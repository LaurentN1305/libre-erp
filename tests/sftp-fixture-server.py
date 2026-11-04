"""Serveur SFTP éphémère sur 127.0.0.1, uniquement pour les tests avec données fictives.
Dépendance de validation : paramiko, à installer hors dépendances produit.
"""
import base64, hashlib, json, os, socket, subprocess, sys, tempfile, threading, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'.validation-deps'/'sftp'))
import paramiko

class Auth(paramiko.ServerInterface):
    def check_auth_password(self, username, password):
        return paramiko.AUTH_SUCCESSFUL if (username,password)==('fixture','fixture-secret') else paramiko.AUTH_FAILED
    def get_allowed_auths(self, username): return 'password'
    def check_channel_request(self, kind, chanid): return paramiko.OPEN_SUCCEEDED if kind=='session' else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

class Files(paramiko.SFTPServerInterface):
    def __init__(self, server, *args, root, **kwargs): super().__init__(server,*args,**kwargs);self.root=Path(root).resolve()
    def path(self, p):
        q=(self.root/p.lstrip('/')).resolve()
        if q!=self.root and self.root not in q.parents: raise OSError(13,'outside fixture')
        return q
    def _error(self,e): return paramiko.SFTPServer.convert_errno(e.errno or 5)
    def canonicalize(self,p): return '/' + str(self.path(p).relative_to(self.root)).replace('\\','/').removeprefix('.')
    def stat(self,p):
        try:return paramiko.SFTPAttributes.from_stat(self.path(p).stat())
        except OSError as e:return self._error(e)
    lstat=stat
    def list_folder(self,p):
        try:
            out=[]
            for q in self.path(p).iterdir():
                a=paramiko.SFTPAttributes.from_stat(q.stat());a.filename=q.name;out.append(a)
            return out
        except OSError as e:return self._error(e)
    def open(self,p,flags,attr):
        try:
            fd=os.open(self.path(p),flags|os.O_BINARY if hasattr(os,'O_BINARY') else flags,0o600)
            f=os.fdopen(fd,'r+b' if flags&os.O_RDWR else 'wb' if flags&os.O_WRONLY else 'rb',buffering=0)
            h=paramiko.SFTPHandle(flags)
            if flags&os.O_WRONLY or flags&os.O_RDWR:h.writefile=f
            if not flags&os.O_WRONLY:h.readfile=f
            return h
        except OSError as e:return self._error(e)
    def mkdir(self,p,attr):
        try:self.path(p).mkdir();return paramiko.SFTP_OK
        except OSError as e:return self._error(e)
    def rmdir(self,p):
        try:self.path(p).rmdir();return paramiko.SFTP_OK
        except OSError as e:return self._error(e)
    def remove(self,p):
        try:self.path(p).unlink();return paramiko.SFTP_OK
        except OSError as e:return self._error(e)
    def rename(self,a,b):
        try:
            if self.path(b).exists():return paramiko.SFTP_FAILURE
            self.path(a).rename(self.path(b));return paramiko.SFTP_OK
        except OSError as e:return self._error(e)

def serve(root):
    key=paramiko.RSAKey.generate(2048)
    sock=socket.socket();sock.bind(('127.0.0.1',0));sock.listen(16)
    print(json.dumps({'port':sock.getsockname()[1],'fingerprint':'SHA256:'+base64.b64encode(hashlib.sha256(key.asbytes()).digest()).decode().rstrip('=')}),flush=True)
    def client(stream):
        t=paramiko.Transport(stream);t.add_server_key(key)
        t.set_subsystem_handler('sftp',paramiko.SFTPServer,Files,root=root)
        try:
            t.start_server(server=Auth())
            while t.is_active():time.sleep(.05)
        finally:t.close()
    while True:
        stream,_=sock.accept();threading.Thread(target=client,args=(stream,),daemon=True).start()

if __name__=='__main__':
    if len(sys.argv)>1 and sys.argv[1]=='--serve':serve(sys.argv[2])
    else:
        with tempfile.TemporaryDirectory(prefix='libre-erp-sftp-fixture-') as root:
            (Path(root)/'libre-erp').mkdir()
            proc=subprocess.Popen([sys.executable,__file__,'--serve',root],stdout=subprocess.PIPE,stderr=open(Path(__file__).resolve().parents[1]/'.tools'/'sftp-fixture-server.log','w'),text=True)
            try:
                settings=json.loads(proc.stdout.readline());env=os.environ.copy()
                env['ERP_SFTP_FIXTURE_PORT']=str(settings['port']);env['ERP_SFTP_FIXTURE_FINGERPRINT']=settings['fingerprint']
                result=subprocess.run(['cargo','test','--locked','--manifest-path','src-tauri/Cargo.toml','--lib','sftp_two_computers','--','--ignored','--nocapture'],env=env)
                sys.exit(result.returncode)
            finally:proc.terminate();proc.wait(timeout=10)
