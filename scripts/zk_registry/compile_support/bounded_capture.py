"""Stage-owned stream sinks with bounded retention and complete byte receipts."""
from contextlib import contextmanager
from contextvars import ContextVar
import hashlib
import json
from pathlib import Path
import threading

_SPEC = ContextVar('capture_sinks', default=None)


def caps():
    return json.loads((Path(__file__).parent/'data/testnet_caps_v2.json').read_text())['output']


class RawSink:
    kind = 'raw'

    def __init__(self, head=None, tail=None, guard='default'):
        profile = caps()['raw']
        self.head_limit = profile['head_bytes'] if head is None else head
        self.tail_limit = profile['tail_bytes'] if tail is None else tail
        self.cap = profile['guard_bytes'] if guard == 'default' else guard
        self.bytes = 0
        self.hasher = hashlib.sha256()
        self.head = bytearray()
        self.tail = bytearray()
        self.capped = False
        self.capture_failed = False

    def feed(self, data):
        self.bytes += len(data)
        self.hasher.update(data)
        need = self.head_limit-len(self.head)
        self.head.extend(data[:max(0, need)])
        if self.tail_limit:
            self.tail.extend(data)
            if len(self.tail)>self.tail_limit:del self.tail[:-self.tail_limit]
        self.capped |= self.cap is not None and self.bytes>self.cap

    def finish(self):
        pass

    @property
    def retained(self):
        if self.bytes<=self.head_limit:return bytes(self.head)
        overlap=max(0,len(self.head)+len(self.tail)-self.bytes)
        return bytes(self.head)+bytes(self.tail[overlap:])

    @property
    def truncated(self):
        return self.bytes>self.head_limit+self.tail_limit

    def receipt(self):
        return dict(kind=self.kind,bytes=self.bytes,sha256=self.hasher.hexdigest(),
                    truncated=self.truncated,capped=self.capped,capture_failed=self.capture_failed)


class SmallSink(RawSink):
    kind = 'small'

    def __init__(self, cap=None):
        cap = caps()['small_bytes'] if cap is None else cap
        super().__init__(cap,0,cap)


class FileSink(RawSink):
    kind = 'file'

    def __init__(self,path,cap=None):
        super().__init__(0,0,caps()['export_bytes'] if cap is None else cap)
        self.record_cap = caps()['export_records']
        self.newlines = 0
        self.partial_record = False
        from zk_registry.safe_fs import open
        self.file = open(path,'xb')
        self.path = path

    @property
    def truncated(self):return self.capped

    def feed(self,data):
        before = self.bytes
        super().feed(data)
        self.newlines += data.count(b'\n')
        if data:self.partial_record = data[-1] != 10
        self.capped |= self.newlines + self.partial_record > self.record_cap
        if before<self.cap:self.file.write(data[:self.cap-before])

    def receipt(self):
        return dict(super().receipt(),records=self.newlines + self.partial_record)

    def finish(self):
        self.file.close()


class MessageSink(RawSink):
    kind = 'messages'

    def __init__(self, message_cap=None,data_cap=None,*,print_name=None,print_line=None,retain_all=False):
        profile=caps()['messages']
        super().__init__(guard=None)
        self.message_cap=profile['count'] if message_cap is None else message_cap
        self.data_cap=profile['data_bytes'] if data_cap is None else data_cap
        self.line_cap=profile['line_bytes']
        self.partial=bytearray()
        self.messages=[]
        self.summary=dict(message_count=0,data_bytes=0,n_errors=0,error_kinds=[],sorry_warnings=0)
        self.print_name,self.print_line,self.retain_all=print_name,print_line,retain_all

    @property
    def truncated(self):return self.capped or self.capture_failed

    def line(self, raw):
        try:
            value=json.loads(raw)
        except (ValueError,UnicodeError):value=None
        if isinstance(value,dict) and isinstance(value.get('data'),str):
            measured_bytes=len(value['data'].encode('utf-8'))
            pos=value.get('pos')
            if pos is not None and (not isinstance(pos,dict) or
                    any(type(pos.get(k)) is not int for k in ('line','column') if k in pos)):
                value=dict(severity='error',data='malformed Lean message',pos=None)
            self.summary['message_count']+=1
            self.summary['data_bytes']+=measured_bytes
            if self.summary['message_count']>self.message_cap or self.summary['data_bytes']>self.data_cap:
                self.capped=True;return
            severity=value.get('severity')
            if severity=='error':
                self.summary['n_errors']+=1
                kind=value.get('kind')
                if isinstance(kind,str) and kind not in self.summary['error_kinds']:self.summary['error_kinds'].append(kind)
            if severity=='warning' and 'sorry' in value['data']:self.summary['sorry_warnings']+=1
            bound=(severity=='information' and (value.get('pos') or {}).get('line')==self.print_line
                   and self.print_name is not None and value['data'].startswith("'"+self.print_name+"' "))
            if self.retain_all or (severity=='error' and self.summary['n_errors']<=10):self.messages.append(value)
            elif bound:
                self.messages=[m for m in self.messages if m.get('severity')!='information']+[value]
        else:
            self.summary['data_bytes']+=len(raw)
            if self.summary['data_bytes']>self.data_cap:self.capped=True
            if self.retain_all and raw.strip() and not self.capped:
                self.summary['message_count']+=1
                self.summary['n_errors']+=1
                if self.summary['message_count']>self.message_cap:self.capped=True;return
                self.messages.append(dict(severity='error',data='non-JSON output: '+raw.decode('utf-8','replace').rstrip('\r\n')[:500],pos=None))

    def feed(self,data):
        super().feed(data)
        if self.capped:return
        self.partial.extend(data)
        while True:
            index=self.partial.find(b'\n')
            if index<0:break
            if index+1>self.line_cap:self.capped=True;self.partial.clear();return
            raw=bytes(self.partial[:index+1]);del self.partial[:index+1]
            self.line(raw)
            if self.capped:self.partial.clear();return
        if len(self.partial)>self.line_cap:self.capped=True;self.partial.clear()

    def finish(self):
        if self.partial and not self.capped:self.line(bytes(self.partial))
        self.partial.clear()

    def receipt(self):
        return dict(super().receipt(),message_count=self.summary['message_count'],data_bytes=self.summary['data_bytes'],
                    log_truncated=RawSink.truncated.fget(self))


@contextmanager
def capture(*,stdout,stderr=None):
    token=_SPEC.set((stdout,stderr))
    try:yield
    finally:
        stdout.finish()
        _SPEC.reset(token)


def configured():
    return _SPEC.get() is not None


def sinks():
    spec=_SPEC.get()
    return (spec[0],spec[1] or RawSink()) if spec else (RawSink(),RawSink())


def drain(pipe,sink,stop=None,channel='stdout'):
    try:
        while True:
            data=pipe.read(65536)
            if not data:break
            sink.feed(data)
            if sink.capped and stop:stop('output-cap:'+channel)
    except Exception:
        sink.capture_failed=True
        if stop:stop('capture-failed:'+channel)
    finally:
        try:
            sink.finish()
            if sink.capped and stop:stop('output-cap:'+channel)
        except (OSError,ValueError):
            sink.capture_failed=True
            if stop:stop('capture-failed:'+channel)
        pipe.close()


class Capture:
    def __init__(self,child,stop):
        self.stdout,self.stderr=sinks()
        self.stop=stop
        self.threads=[threading.Thread(target=drain,args=(pipe,sink,stop,channel),daemon=True)
                      for pipe,sink,channel in [(child.stdout,self.stdout,'stdout'),(child.stderr,self.stderr,'stderr')]]
        for thread in self.threads:thread.start()

    def join(self,timeout=2):
        for thread in self.threads:thread.join(timeout)
        for thread,sink,channel in zip(self.threads,[self.stdout,self.stderr],['stdout','stderr']):
            if thread.is_alive():
                sink.capture_failed=True
                self.stop('capture-failed:'+channel)

    def apply(self,result):
        result.streams={channel:sink.receipt() for channel,sink in [('stdout',self.stdout),('stderr',self.stderr)]}
        result.output_capped=result.killed_by.startswith('output-cap:')
        result.out=self.stdout.retained.decode('utf-8','replace')
        result.stderr=self.stderr.retained.decode('utf-8','replace')
        if isinstance(self.stdout,MessageSink):
            result.messages=[] if self.stdout.capped else self.stdout.messages
            result.message_summary=None if self.stdout.capped else self.stdout.summary
        return result
