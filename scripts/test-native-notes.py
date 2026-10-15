#!/usr/bin/env python3
"""Native WebKit -> GTK clipboard regression on an isolated Xvfb display.

Uses a temporary synthetic library, never the user's recordings or clipboard.
"""
import ctypes
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import time

root=Path(__file__).resolve().parents[1]
x11=ctypes.CDLL('libX11.so.6')
xtst=ctypes.CDLL('libXtst.so.6')
x11.XOpenDisplay.restype=ctypes.c_void_p
x11.XFlush.argtypes=[ctypes.c_void_p]
x11.XKeysymToKeycode.argtypes=[ctypes.c_void_p,ctypes.c_ulong]
x11.XKeysymToKeycode.restype=ctypes.c_uint
xtst.XTestFakeMotionEvent.argtypes=[ctypes.c_void_p,ctypes.c_int,ctypes.c_int,ctypes.c_int,ctypes.c_ulong]
xtst.XTestFakeButtonEvent.argtypes=[ctypes.c_void_p,ctypes.c_uint,ctypes.c_int,ctypes.c_ulong]
xtst.XTestFakeKeyEvent.argtypes=[ctypes.c_void_p,ctypes.c_uint,ctypes.c_int,ctypes.c_ulong]
display=x11.XOpenDisplay(None)
assert display,'Run on an isolated Xvfb display'

def click(x,y):
    xtst.XTestFakeMotionEvent(display,-1,x,y,0)
    xtst.XTestFakeButtonEvent(display,1,1,0)
    xtst.XTestFakeButtonEvent(display,1,0,0)
    x11.XFlush(display)
    time.sleep(1)

def snapshot(name):
    subprocess.run(['import','-window','root',f'/tmp/voxloom-native-notes-{name}.png'],check=True)

with tempfile.TemporaryDirectory(prefix='voxloom-native-notes-') as directory:
    data=Path(directory)
    with sqlite3.connect(data/'library.db') as db:
        for migration in ['001.sql','002.sql']:
            db.executescript((root/'src-tauri/migrations'/migration).read_text())
        db.execute("INSERT INTO meetings(id,title,created_at,duration_ms,status,summary) VALUES(?,?,?,?,?,?)",('fixture','Clipboard fixture',int(time.time()),60000,'ready','Overview\nThe workshop covers privacy.\n\nAction items\n• **Sarah** sends the guide on Friday at 09:30.'))
    env=dict(os.environ,VOXLOOM_DATA_DIR=directory,VOXLOOM_CACHE_DIR=directory+'/cache')
    with open('/tmp/voxloom-native-notes.log','w') as log:
        app=subprocess.Popen([sys.argv[1]],env=env,stdout=log,stderr=log)
        try:
            time.sleep(5)
            assert app.poll() is None
            key=x11.XKeysymToKeycode(display,0xff1b)
            xtst.XTestFakeKeyEvent(display,key,1,0);xtst.XTestFakeKeyEvent(display,key,0,0);x11.XFlush(display);time.sleep(.5)
            click(105,258)
            snapshot('library')
            click(460,378)
            snapshot('detail')
            click(370,384)
            snapshot('summary')
            click(965,508)
            snapshot('copied')
            gtk=ctypes.CDLL('libgtk-3.so.0')
            gdk=ctypes.CDLL('libgdk-3.so.0')
            gdk.gdk_atom_intern_static_string.argtypes=[ctypes.c_char_p]
            gdk.gdk_atom_intern_static_string.restype=ctypes.c_void_p
            gtk.gtk_init_check.argtypes=[ctypes.c_void_p,ctypes.c_void_p]
            assert gtk.gtk_init_check(None,None)
            gtk.gtk_clipboard_get.argtypes=[ctypes.c_void_p];gtk.gtk_clipboard_get.restype=ctypes.c_void_p
            clipboard=gtk.gtk_clipboard_get(gdk.gdk_atom_intern_static_string(b'CLIPBOARD'))
            gtk.gtk_clipboard_wait_for_text.argtypes=[ctypes.c_void_p];gtk.gtk_clipboard_wait_for_text.restype=ctypes.c_char_p
            plain=(gtk.gtk_clipboard_wait_for_text(clipboard) or b'').decode()
            assert 'Sarah sends the guide on Friday at 09:30.' in plain,repr(plain)
            assert '**' not in plain
            gtk.gtk_clipboard_wait_for_contents.argtypes=[ctypes.c_void_p,ctypes.c_void_p];gtk.gtk_clipboard_wait_for_contents.restype=ctypes.c_void_p
            selection=gtk.gtk_clipboard_wait_for_contents(clipboard,gdk.gdk_atom_intern_static_string(b'text/html'))
            assert selection,'HTML clipboard format missing'
            gtk.gtk_selection_data_get_data.argtypes=[ctypes.c_void_p];gtk.gtk_selection_data_get_data.restype=ctypes.c_void_p
            gtk.gtk_selection_data_get_length.argtypes=[ctypes.c_void_p];gtk.gtk_selection_data_get_length.restype=ctypes.c_int
            html=ctypes.string_at(gtk.gtk_selection_data_get_data(selection),gtk.gtk_selection_data_get_length(selection)).decode(errors='replace')
            assert '<h3' in html and '<strong>Sarah</strong>' in html,html
            click(179,791)
            app.wait(timeout=10)
            print({'native_copy_plain':'PASS','native_copy_html':'PASS','quit_exit_code':app.returncode})
        finally:
            if app.poll() is None:app.terminate();app.wait(timeout=10)
