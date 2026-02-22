import os
import shutil
import threading
import tkinter as tk
from tkinter import ttk, messagebox
import ctypes

# Configuration: Folders to Monitor
# Format: ("Display Name", "Path", "CanClean")
FOLDERS_TO_MONITOR = [
    ("User Temp Files", os.path.expandvars(r"%TEMP%"), True),
    ("Windows Temp", r"C:\Windows\Temp", True), # Might need Admin
    ("VS Code Workspace", os.path.expandvars(r"%APPDATA%\Code\User\workspaceStorage"), True),
    ("VS Code Cached Data", os.path.expandvars(r"%APPDATA%\Code\CachedData"), True),
    ("VS Code Extensions Installer", os.path.expandvars(r"%APPDATA%\Code\CachedExtensionVSIXs"), True),
    ("CapCut Cache", os.path.expandvars(r"%LOCALAPPDATA%\CapCut\User Data\Cache"), True),
    ("CapCut Pre-Render", os.path.expandvars(r"%LOCALAPPDATA%\CapCut\segmentPrerenderCache"), True),
    ("Docker Data", os.path.expandvars(r"%LOCALAPPDATA%\Docker"), False), # Monitor Only
    ("Pip Cache (Drive D)", r"D:\.pip_cache", True),
    ("Dev Tools (Drive D)", r"D:\.dev_tools", False) # Monitor Only
]

class CacheMonitorApp:
    def __init__(self, root):
        self.root = root
        self.root.title("System Cache Monitor")
        self.root.geometry("700x550")
        self.root.configure(bg="#1e1e1e")
        
        # Style
        self.style = ttk.Style()
        self.style.theme_use('clam')
        self.style.configure("Treeview", 
                           background="#2d2d2d", 
                           foreground="white", 
                           fieldbackground="#2d2d2d", 
                           rowheight=30)
        self.style.configure("Treeview.Heading", 
                           background="#333333", 
                           foreground="white", 
                           font=('Segoe UI', 10, 'bold'))
        self.style.map("Treeview", background=[('selected', '#007acc')])
        
        # Header
        header_frame = tk.Frame(root, bg="#1e1e1e")
        header_frame.pack(fill=tk.X, padx=20, pady=20)
        
        title_lbl = tk.Label(header_frame, text="System Storage Monitor", 
                           font=("Segoe UI", 18, "bold"), 
                           bg="#1e1e1e", fg="white")
        title_lbl.pack(side=tk.LEFT)
        
        scan_btn = tk.Button(header_frame, text="Rescan System", 
                           command=self.start_scan,
                           bg="#007acc", fg="white", 
                           font=("Segoe UI", 10), relief=tk.FLAT, padx=15, pady=5)
        scan_btn.pack(side=tk.RIGHT)

        # List Area
        list_frame = tk.Frame(root, bg="#1e1e1e")
        list_frame.pack(fill=tk.BOTH, expand=True, padx=20, pady=5)
        
        columns = ("name", "path", "size", "status")
        self.tree = ttk.Treeview(list_frame, columns=columns, show="headings", selectmode="none")
        
        self.tree.heading("name", text="Folder Name")
        self.tree.heading("path", text="Path")
        self.tree.heading("size", text="Size")
        self.tree.heading("status", text="Health")
        
        self.tree.column("name", width=150)
        self.tree.column("path", width=250)
        self.tree.column("size", width=100, anchor="e")
        self.tree.column("status", width=100, anchor="center")
        
        # Scrollbar
        scrollbar = ttk.Scrollbar(list_frame, orient=tk.VERTICAL, command=self.tree.yview)
        self.tree.configure(yscroll=scrollbar.set)
        
        self.tree.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        scrollbar.pack(side=tk.RIGHT, fill=tk.Y)
        
        # Action Area
        action_frame = tk.Frame(root, bg="#1e1e1e")
        action_frame.pack(fill=tk.X, padx=20, pady=20)
        
        info_lbl = tk.Label(action_frame, text="Select an item above to see actions (Not implemented in this auto-version)", 
                          bg="#1e1e1e", fg="#888888")
        # Instead of selection, we use a global Clean ALL button for simplicity or right click context
        # For this version: "Clean All Safe Cache" button
        
        self.clean_btn = tk.Button(action_frame, text="Clean All Safe Cache (> 0 MB)", 
                                 command=self.clean_all_safe,
                                 bg="#d32f2f", fg="white", 
                                 font=("Segoe UI", 10, "bold"), relief=tk.FLAT, padx=20, pady=8)
        self.clean_btn.pack(side=tk.RIGHT)
        
        self.status_lbl = tk.Label(action_frame, text="Ready", bg="#1e1e1e", fg="#aaaaaa")
        self.status_lbl.pack(side=tk.LEFT, pady=10)

        # Start initial scan
        self.start_scan()

    def get_size(self, path):
        total_size = 0
        try:
            for dirpath, dirnames, filenames in os.walk(path):
                for f in filenames:
                    fp = os.path.join(dirpath, f)
                    if not os.path.islink(fp):
                        total_size += os.path.getsize(fp)
        except Exception:
            pass
        return total_size

    def format_size(self, size):
        for unit in ['B', 'KB', 'MB', 'GB', 'TB']:
            if size < 1024.0:
                return f"{size:.2f} {unit}"
            size /= 1024.0
        return f"{size:.2f} TB"

    def start_scan(self):
        self.status_lbl.config(text="Scanning in progress...")
        self.tree.delete(*self.tree.get_children())
        threading.Thread(target=self.scan_dirs, daemon=True).start()

    def scan_dirs(self):
        self.scan_results = []
        for name, path, can_clean in FOLDERS_TO_MONITOR:
            if os.path.exists(path):
                size = self.get_size(path)
                
                # Determine status color
                status = "Good"
                tag = "green"
                size_mb = size / (1024*1024)
                
                if size_mb > 2048: # > 2GB
                    status = "Heavy"
                    tag = "red"
                elif size_mb > 500: # > 500MB
                    status = "Warning"
                    tag = "yellow"
                
                self.scan_results.append((name, path, size, self.format_size(size), status, tag, can_clean))
            else:
                self.scan_results.append((name, path, 0, "Not Found", "Empty", "gray", False))
                
        self.root.after(0, self.update_ui_after_scan)

    def update_ui_after_scan(self):
        for item in self.scan_results:
            name, path, raw_size, fmt_size, status, tag, can_clean = item
            
            display_name = name
            if not can_clean and raw_size > 0:
                display_name += " (Monitor Only)"
            
            # Insert into tree
            t_id = self.tree.insert("", "end", values=(display_name, path, fmt_size, status))
            
            # Colorizing
            if tag == "red":
                self.tree.tag_configure(t_id, foreground="#ff4444")
            elif tag == "yellow":
                 self.tree.tag_configure(t_id, foreground="#ffbb33")
            elif tag == "green":
                 self.tree.tag_configure(t_id, foreground="#00C851")
            elif tag == "gray":
                 self.tree.tag_configure(t_id, foreground="#666666")
                 
        self.status_lbl.config(text=f"Scan complete. Found {len(self.scan_results)} items.")

    def clean_all_safe(self):
        if not messagebox.askyesno("Confirm Clean", "Are you sure you want to clean all safe folders?\n\nThis will permanently delete temporary files and caches.\n(Monitor Only folders like Docker will NOT be touched)"):
            return
            
        self.status_lbl.config(text="Cleaning...")
        threading.Thread(target=self.perform_clean, daemon=True).start()

    def perform_clean(self):
        freed = 0
        errors = 0
        
        for name, path, can_clean in FOLDERS_TO_MONITOR:
            if can_clean and os.path.exists(path):
                try:
                    # Method: Delete contents, keep folder
                    for item in os.listdir(path):
                        item_path = os.path.join(path, item)
                        try:
                            if os.path.isfile(item_path) or os.path.islink(item_path):
                                os.unlink(item_path)
                                freed += 1 # Just counting items roughly, usually we count bytes
                            elif os.path.isdir(item_path):
                                shutil.rmtree(item_path)
                        except Exception as e:
                            errors += 1
                except Exception:
                    errors += 1
        
        self.root.after(0, lambda: self.finish_clean(errors))

    def finish_clean(self, errors):
        msg = "Cleanup complete!"
        if errors > 0:
            msg += f"\n(Skipped {errors} files in use/access denied)"
        
        messagebox.showinfo("Done", msg)
        self.start_scan()

if __name__ == "__main__":
    # Hide Console if run as script, but we are running via pythonw usually
    root = tk.Tk()
    app = CacheMonitorApp(root)
    root.mainloop()
