"""Synthetic traffic exclusively for disposable documentation screenshots."""
import hashlib
import sqlite3
import time


def seed(root):
    today=int(time.time())//86400
    conn=sqlite3.connect(root/'state/state.db')
    for offset in range(1,31):
        day=today-offset
        downloads=44+(31-offset)*3+(offset*19)%43
        for kind,count,size in [('packages',downloads,62000),('repository',downloads*4,7800),('assets',downloads*2,14000),('pages',downloads//2,9300)]:
            conn.execute('INSERT OR REPLACE INTO analytics_daily(day,kind,requests,downloads,ranges,bytes,errors,not_modified,interrupted) VALUES(?,?,?,?,?,?,?,?,?)',
                         (day,kind,count,downloads if kind=='packages' else 0,offset%3 if kind=='packages' else 0,count*size,offset%4,offset%7,0))
        for index in range(downloads//2):
            digest=hashlib.sha256(f'disposable-client-{index+offset}'.encode()).digest()
            conn.execute('INSERT OR IGNORE INTO analytics_clients(day,client) VALUES(?,?)',(day,digest))
        for name,factor in [('xxc-fixture',2),('xxc-browser-fixture',3)]:
            count=downloads//factor
            conn.execute('INSERT OR REPLACE INTO analytics_assets(day,path,kind,package,requests,downloads,bytes) VALUES(?,?,?,?,?,?,?)',
                         (day,f'/repo/pool/main/x/{name}/{name}_1.0_all.deb','packages',name,count,count,count*62000))
        for path,kind in [('/repo/dists/zerotrust/InRelease','repository'),('/static/site.css','assets'),('/static/site.js','assets')]:
            conn.execute('INSERT OR REPLACE INTO analytics_assets(day,path,kind,package,requests,downloads,bytes) VALUES(?,?,?,?,?,?,?)',
                         (day,path,kind,'',downloads*2,0,downloads*14000))
    conn.commit();conn.close()
