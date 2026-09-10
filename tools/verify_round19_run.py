import subprocess, time, os, sys, signal

exe = r'D:\RdataStation\RDS\RdataStation-v2\target\debug\rds-app.exe'
if not os.path.exists(exe):
    print('EXE NOT FOUND:', exe)
    sys.exit(2)

proc = subprocess.Popen(
    [exe],
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
    encoding='utf-8',
    errors='replace',
)
time.sleep(8)

alive = proc.poll() is None
print('process alive after 8s:', alive)
print('exit code:', proc.poll())

if alive:
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)
    print('terminated after verify')

# 读取残留输出
out, err = proc.communicate(timeout=3)
print('--- stdout tail ---')
print(out[-2000:] if out else '(empty)')
print('--- stderr tail ---')
print(err[-2000:] if err else '(empty)')
