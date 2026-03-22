<script setup lang="ts">
import { ref, onMounted, computed, watch, nextTick, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { register, unregisterAll } from "@tauri-apps/plugin-global-shortcut";
import { open } from "@tauri-apps/plugin-dialog";
import { convertFileSrc } from "@tauri-apps/api/core";
import draggable from "vuedraggable";

// Custom directive for input auto-focus
const vFocus = {
  mounted: (el: HTMLElement) => el.focus()
};

interface Book {
  id: string; // The path is unique
  name: string;
  files: string[];
  currentIndex: number;
  currentTime: number;
}

interface ScanResult {
  name: string;
  path: string;
  files: string[];
}

const library = ref<Book[]>([]);
const currentBookId = ref<string>("");
const showSettings = ref(false);
const recordingKeyFor = ref("");

type Settings = {
  minimizeToTray: boolean;
  enableGlobalShortcuts: boolean;
  enableLocalShortcuts: boolean;
  gsPlayPause: string;
  gsNext: string;
  gsPrev: string;
  gsForward: string;
  gsBackward: string;
};

const recordKey = (e: KeyboardEvent, keyName: keyof Settings) => {
  const keys = [];
  if (e.ctrlKey || e.metaKey) keys.push("CommandOrControl");
  if (e.altKey) keys.push("Alt");
  if (e.shiftKey) keys.push("Shift");
  
  if (["Control", "Alt", "Shift", "Meta"].includes(e.key)) return;
  
  let key = e.key;
  if (e.code.startsWith("Key")) key = e.code.replace("Key", "");
  else if (e.code.startsWith("Digit")) key = e.code.replace("Digit", "");
  else if (e.code === "Space") key = "Space";
  else if (e.code === "ArrowUp") key = "Up";
  else if (e.code === "ArrowDown") key = "Down";
  else if (e.code === "ArrowLeft") key = "Left";
  else if (e.code === "ArrowRight") key = "Right";
  else key = key.toUpperCase();
  
  keys.push(key);
  (settings.value as any)[keyName] = keys.join("+");
  recordingKeyFor.value = "";
  (e.target as HTMLElement)?.blur();
};
const settings = ref({
  minimizeToTray: true,
  enableGlobalShortcuts: false,
  enableLocalShortcuts: true,
  gsPlayPause: "CommandOrControl+Shift+Space",
  gsNext: "CommandOrControl+Shift+Right",
  gsPrev: "CommandOrControl+Shift+Left",
  gsForward: "CommandOrControl+Shift+Up",
  gsBackward: "CommandOrControl+Shift+Down",
});

let unlistenDrop: UnlistenFn | null = null;
let unlistenClose: UnlistenFn | null = null;
let unlistenTray: UnlistenFn | null = null;
const expandedBookId = ref<string>("");
const editingBookId = ref<string>("");

const currentTime = ref(0);
const playbackRate = ref(1.0);
const isPlaying = ref(false);
const duration = ref(0);
const isDragging = ref(false);

const audioPlayer = ref<HTMLAudioElement | null>(null);
const progressBar = ref<HTMLElement | null>(null);

// Get the active book object
const activeBook = computed(() => library.value.find(b => b.id === currentBookId.value));

const currentAudioPath = computed(() => {
  const book = activeBook.value;
  if (book && book.currentIndex >= 0 && book.currentIndex < book.files.length) {
    return book.files[book.currentIndex];
  }
  return "";
});

const currentAudioName = computed(() => {
  if (!currentAudioPath.value) return "独立音频轨道待命中";
  const parts = currentAudioPath.value.split(/[/\\]/);
  return parts[parts.length - 1].replace(/\.[^/.]+$/, ""); 
});

const currentBookName = computed(() => {
  return activeBook.value?.name || "";
});

const currentAudioUrl = computed(() => {
  if (currentAudioPath.value) {
    return convertFileSrc(currentAudioPath.value);
  }
  return "";
});

onMounted(async () => {
  const savedSettings = localStorage.getItem("audiobook_settings");
  if (savedSettings) {
    try { Object.assign(settings.value, JSON.parse(savedSettings)); } catch {}
  }
  await bindGlobalShortcuts();

  const savedState = localStorage.getItem("audiobook_library_v2");
  if (savedState) {
    try {
      const state = JSON.parse(savedState);
      library.value = state.library || [];
      currentBookId.value = state.currentBookId || "";
      playbackRate.value = state.playbackRate || 1.0;
      if (activeBook.value) {
        expandedBookId.value = activeBook.value.id;
        initAudio();
      }
    } catch {}
  }

  window.addEventListener("keydown", handleKeydown);

  unlistenDrop = await listen("tauri://drop", async (event: any) => {
    const paths = event.payload.paths || event.payload;
    if (Array.isArray(paths)) {
      for (const p of paths) await scanSubDirs(p);
    }
  });

  unlistenTray = await listen<string>("tray-action", (event) => {
    const action = event.payload;
    if (action === "play_pause") togglePlay();
    else if (action === "prev") playPrev();
    else if (action === "next") playNext();
  });

  unlistenClose = await getCurrentWindow().onCloseRequested(async (e) => {
    e.preventDefault();
    if (settings.value.minimizeToTray) {
      await getCurrentWindow().hide();
    } else {
      await invoke("exit_app");
    }
  });
});

onUnmounted(() => {
  window.removeEventListener("keydown", handleKeydown);
  if (unlistenDrop) unlistenDrop();
  if (unlistenClose) unlistenClose();
  if (unlistenTray) unlistenTray();
});

const handleKeydown = (e: KeyboardEvent) => {
  if (e.target instanceof HTMLInputElement || !settings.value.enableLocalShortcuts || showSettings.value) return;
  if (e.code === "Space") { e.preventDefault(); togglePlay(); }
  if (e.code === "ArrowRight") { e.preventDefault(); skipTime(15); }
  if (e.code === "ArrowLeft") { e.preventDefault(); skipTime(-15); }
};

const bindGlobalShortcuts = async () => {
  localStorage.setItem("audiobook_settings", JSON.stringify(settings.value));
  try {
    await unregisterAll();
    if (settings.value.enableGlobalShortcuts) {
      if (settings.value.gsPlayPause) await register(settings.value.gsPlayPause, e => { if(e.state==='Pressed') togglePlay(); });
      if (settings.value.gsNext) await register(settings.value.gsNext, e => { if(e.state==='Pressed') playNext(); });
      if (settings.value.gsPrev) await register(settings.value.gsPrev, e => { if(e.state==='Pressed') playPrev(); });
      if (settings.value.gsForward) await register(settings.value.gsForward, e => { if(e.state==='Pressed') skipTime(15); });
      if (settings.value.gsBackward) await register(settings.value.gsBackward, e => { if(e.state==='Pressed') skipTime(-15); });
    }
  } catch(e) { console.warn(e); }
};

watch([library, currentBookId, playbackRate], () => {
  saveState();
}, { deep: true });

const saveState = () => {
  // If active book, ensure its time is updated before save
  if (activeBook.value && audioPlayer.value && !isDragging.value) {
    activeBook.value.currentTime = audioPlayer.value.currentTime;
  }
  localStorage.setItem("audiobook_library_v2", JSON.stringify({
    library: library.value,
    currentBookId: currentBookId.value,
    playbackRate: playbackRate.value
  }));
};

const selectFolder = async () => {
  const selected = await open({
    directory: true,
    multiple: true,
  });
  if (selected) {
    const folders = Array.isArray(selected) ? selected : [selected];
    for (const f of folders) await scanSubDirs(f);
  }
};

const scanSubDirs = async (dir: string) => {
  try {
    const results: ScanResult[] = await invoke("scan_audio_files", { dirPath: dir });
    if (results.length > 0) {
      results.forEach(res => {
        const existing = library.value.find(b => b.id === res.path);
        if (existing) {
          // Update files but keep progress/name
          existing.files = res.files;
        } else {
          // Add new book
          library.value.push({
            id: res.path, // Use absolute path as unique ID
            name: res.name || getFileName(res.path),
            files: res.files,
            currentIndex: 0,
            currentTime: 0,
          });
        }
      });
      // Optionally auto expand the first new result
      expandedBookId.value = results[0].path;
      // If nothing is playing, play the first one
      if (!currentBookId.value) {
        currentBookId.value = results[0].path;
      }
    }
  } catch (err) {
    console.error("Scan failed:", err);
  }
};

const initAudio = () => {
  if (audioPlayer.value && activeBook.value) {
    audioPlayer.value.currentTime = activeBook.value.currentTime;
    audioPlayer.value.playbackRate = playbackRate.value;
  }
};

const toggleExpand = (id: string) => {
  if (expandedBookId.value === id) {
    expandedBookId.value = "";
  } else {
    expandedBookId.value = id;
  }
};

const startEditing = (id: string) => {
  editingBookId.value = id;
};

const stopEditing = () => {
  editingBookId.value = "";
  saveState();
};

const togglePlay = () => {
  if (!audioPlayer.value || !activeBook.value) return;
  if (isPlaying.value) {
    audioPlayer.value.pause();
  } else {
    audioPlayer.value.play();
  }
};

const playSpecific = (bookId: string, index: number) => {
  const book = library.value.find(b => b.id === bookId);
  if (!book) return;
  
  if (currentBookId.value !== bookId) {
    // Save current book time before switching
    if (activeBook.value && audioPlayer.value) {
      activeBook.value.currentTime = audioPlayer.value.currentTime;
    }
    currentBookId.value = bookId;
  }
  
  // Changing track resets the book progress to 0 for that new track
  if (book.currentIndex !== index) {
    book.currentIndex = index;
    book.currentTime = 0; 
  }
  
  nextTick(() => {
    if (audioPlayer.value) {
      audioPlayer.value.currentTime = book.currentTime;
      playAudio();
    }
  });
};

const playNext = () => {
  const book = activeBook.value;
  if (book && book.currentIndex < book.files.length - 1) {
    book.currentIndex++;
    book.currentTime = 0;
    nextTick(() => {
      if (audioPlayer.value) {
        audioPlayer.value.currentTime = 0;
        playAudio();
      }
    });
  }
};

const playPrev = () => {
  const book = activeBook.value;
  if (book && book.currentIndex > 0) {
    book.currentIndex--;
    book.currentTime = 0;
    nextTick(() => {
      if (audioPlayer.value) {
        audioPlayer.value.currentTime = 0;
        playAudio();
      }
    });
  }
};

const playAudio = () => {
  if (audioPlayer.value) {
    audioPlayer.value.play().catch(console.error);
  }
};

// Seek Bar Logics ---
const startSeek = (e: MouseEvent) => {
  if (duration.value === 0 || !activeBook.value) return;
  isDragging.value = true;
  updateSeek(e);
  document.addEventListener("mousemove", updateSeek);
  document.addEventListener("mouseup", endSeek);
};

const updateSeek = (e: MouseEvent) => {
  if (!progressBar.value || duration.value === 0 || !activeBook.value) return;
  const rect = progressBar.value.getBoundingClientRect();
  let x = e.clientX - rect.left;
  if (x < 0) x = 0;
  if (x > rect.width) x = rect.width;
  activeBook.value.currentTime = (x / rect.width) * duration.value;
};

const endSeek = (e: MouseEvent) => {
  document.removeEventListener("mousemove", updateSeek);
  document.removeEventListener("mouseup", endSeek);
  if (audioPlayer.value && activeBook.value) {
    audioPlayer.value.currentTime = activeBook.value.currentTime;
  }
  setTimeout(() => {
    isDragging.value = false;
  }, 100);
};

const skipTime = (amount: number) => {
  let time = (audioPlayer.value ? audioPlayer.value.currentTime : (activeBook.value?.currentTime || 0)) + amount;
  if (time < 0) time = 0;
  if (duration.value > 0 && time > duration.value) time = duration.value;
  
  if (activeBook.value) activeBook.value.currentTime = time;
  if (audioPlayer.value) audioPlayer.value.currentTime = time;
};
// --------------------

const onTimeUpdate = () => {
  if (audioPlayer.value && !isDragging.value && activeBook.value) {
    activeBook.value.currentTime = audioPlayer.value.currentTime;
    if (Math.floor(activeBook.value.currentTime) % 5 === 0) {
      saveState();
    }
  }
};

const onLoadedMetadata = () => {
  if (audioPlayer.value) {
    duration.value = audioPlayer.value.duration;
    if (activeBook.value && !isDragging.value) {
       audioPlayer.value.currentTime = activeBook.value.currentTime;
    }
  }
};

const formatTime = (seconds: number) => {
  if (isNaN(seconds) || seconds === Infinity) return "00:00";
  const m = Math.floor(seconds / 60).toString().padStart(2, "0");
  const s = Math.floor(seconds % 60).toString().padStart(2, "0");
  return `${m}:${s}`;
};

const getFileName = (path: string) => {
  const parts = path.split(/[/\\]/);
  return parts[parts.length - 1];
};

const setRate = (rate: number) => {
  playbackRate.value = rate;
  if (audioPlayer.value) {
    audioPlayer.value.playbackRate = rate;
    saveState();
  }
};

const removeBook = (id: string) => {
  library.value = library.value.filter(b => b.id !== id);
  if (currentBookId.value === id) currentBookId.value = "";
  if (expandedBookId.value === id) expandedBookId.value = "";
};
</script>

<template>
  <div class="flex h-screen overflow-hidden bg-[#050505] text-gray-300 font-sans selection:bg-amber-500/30 text-sm">
    
    <!-- 左侧面板: 书库阵列 -->
    <div class="w-80 flex flex-col border-r border-[#1f1f1f] bg-[#0a0a0a] z-10 m-3 mr-0 shadow-lg">
      
      <!-- 头部控制端 -->
      <div class="p-5 border-b border-[#1f1f1f] bg-[#0f0f0f]">
        <div class="flex items-center justify-between mb-4">
          <div class="font-tech text-base text-amber-500 tracking-wider flex items-center gap-2 font-bold">
            <div class="w-1.5 h-1.5 bg-amber-500 animate-pulse"></div>
            有声书播放器
          </div>
          <button @click="showSettings = true" class="text-[11px] font-bold text-gray-400 hover:text-amber-500 transition-colors font-tech cursor-pointer">
            [ 设置 ]
          </button>
        </div>
        
        <button 
          @click="selectFolder"
          class="w-full relative group bg-[#111] hover:bg-amber-500 text-gray-300 hover:text-black border border-[#333] hover:border-amber-500 py-2 transition-colors cursor-pointer font-bold tracking-widest text-[13px]"
        >
          + 添加书籍 (支持拖拽目录)
        </button>
      </div>

      <!-- 播放列表元数据 -->
      <div class="px-5 py-2 text-[11px] text-gray-400 font-tech border-b border-[#1f1f1f] flex justify-between bg-[#080808]">
        <span>书库列表</span>
        <span class="text-amber-500/80">共 {{ library.length }} 本</span>
      </div>

      <!-- 多书籍书架容器 -->
      <div class="flex-1 overflow-y-auto p-2 custom-scrollbar relative bg-[#0a0a0a]">
        
        <div v-for="book in library" :key="book.id" class="mb-2 border border-[#222] bg-[#0c0c0c] flex flex-col">
          <!-- Book Header (Accordion Toggle) -->
          <div 
            class="px-3 py-2.5 cursor-pointer flex justify-between items-center hover:bg-[#1a1305]/50 select-none group transition-colors"
            :class="expandedBookId === book.id ? 'bg-[#1a1305] border-b border-amber-500/20' : 'bg-[#111]'"
            @click="toggleExpand(book.id)"
          >
            <div class="flex items-center gap-2 flex-1 overflow-hidden" @dblclick.stop="startEditing(book.id)">
              <span class="text-amber-500 font-tech w-4 text-center font-bold">{{ expandedBookId === book.id ? '-' : '+' }}</span>
              <input 
                v-if="editingBookId === book.id" 
                v-model="book.name" 
                @blur="stopEditing"
                @keyup.enter="stopEditing"
                @click.stop
                v-focus
                class="bg-transparent border-b border-amber-500 outline-none text-amber-500 font-bold w-full text-[13px] px-1"
              />
              <span 
                v-else 
                class="font-bold truncate text-[13px] tracking-wide"
                :class="currentBookId === book.id ? 'text-amber-500' : 'text-gray-300 group-hover:text-amber-400'"
                :title="'双击重命名: ' + book.name"
              >
                {{ book.name }}
              </span>
            </div>
            
            <div class="flex items-center gap-2 pl-2">
              <span class="text-[10px] text-gray-600 font-tech">{{ book.files.length }}段</span>
              <button @click.stop="removeBook(book.id)" class="text-gray-600 hover:text-red-500 transition-colors font-bold px-1" title="从此列表中移除">X</button>
            </div>
          </div>

          <!-- Track List (Draggable) -->
          <div v-if="expandedBookId === book.id" class="flex flex-col border-t border-[#1f1f1f]/50 bg-[#0f0f0f]">
            <draggable 
              v-model="book.files" 
              item-key="id" 
              class="flex flex-col py-1 pointer-events-auto"
              ghost-class="opacity-30"
              animation="200"
            >
              <template #item="{ element, index }">
                <div 
                  @click="playSpecific(book.id, index)"
                  :class="[
                    'group flex items-center gap-3 px-3 py-2 cursor-pointer transition-colors border-l-[3px] text-[12px] tracking-wide',
                    currentBookId === book.id && book.currentIndex === index 
                      ? 'border-amber-500 text-amber-500 bg-[#1a1305]' 
                      : 'border-transparent hover:bg-[#1a1a1a] hover:border-gray-500 text-gray-500'
                  ]"
                >
                  <div class="font-tech opacity-40 text-[9px] w-4 text-right shrink-0">{{ String(index + 1).padStart(2, '0') }}</div>
                  
                  <div class="flex-1 truncate font-medium" :title="getFileName(element)">
                    {{ getFileName(element) }}
                  </div>

                  <!-- 播放状态指示器 -->
                  <div v-if="currentBookId === book.id && book.currentIndex === index && isPlaying" class="w-2.5 flex justify-between h-2.5 items-end shrink-0">
                    <div class="w-[1px] bg-amber-500 h-full animate-bounce"></div>
                    <div class="w-[1px] bg-amber-500 h-[60%] animate-bounce" style="animation-delay: 0.1s"></div>
                    <div class="w-[1px] bg-amber-500 h-[80%] animate-bounce" style="animation-delay: 0.2s"></div>
                  </div>
                </div>
              </template>
            </draggable>
          </div>

        </div>

        <div v-if="library.length === 0" class="absolute inset-0 flex flex-col items-center justify-center text-gray-600 space-y-3 font-tech text-xs tracking-widest pointer-events-none">
          <div class="flex gap-2 items-center">
            <span class="w-1.5 h-1.5 bg-gray-600"></span> 资料库空载
          </div>
          <div class="text-[10px] text-gray-700 w-3/4 text-center">点击上方按钮扫描本地层级或导入文件</div>
        </div>
      </div>
    </div>

    <!-- 右侧主面板: 核心显示终端 -->
    <div class="flex-1 flex flex-col relative z-10 m-3 border border-[#1f1f1f] bg-[#0a0a0a] shadow-lg">
      
      <!-- 顶栏状态 -->
      <div class="h-8 border-b border-[#1f1f1f] px-4 flex items-center justify-between text-[11px] text-gray-500 font-tech bg-[#0f0f0f]">
        <div class="flex gap-4">
          <span class="font-bold text-gray-400">悦听有声书</span>
        </div>
        <div class="flex items-center gap-2">
          <span>当前状态:</span>
          <span :class="isPlaying ? 'text-amber-500' : 'text-gray-500'">{{ isPlaying ? '正在播放' : '已暂停' }}</span>
        </div>
      </div>

      <!-- 中心数据显示 -->
      <div class="flex-1 flex flex-col items-center justify-center p-8 relative overflow-hidden bg-[radial-gradient(ellipse_at_center,_var(--tw-gradient-stops))] from-[#111] to-[#050505]">
        <!-- 科技感准星修饰 -->
        <div class="absolute top-10 left-10 w-6 h-6 border-l-2 border-t-2 border-[#333]"></div>
        <div class="absolute top-10 right-10 w-6 h-6 border-r-2 border-t-2 border-[#333]"></div>
        <div class="absolute bottom-10 left-10 w-6 h-6 border-l-2 border-b-2 border-[#333]"></div>
        <div class="absolute bottom-10 right-10 w-6 h-6 border-r-2 border-b-2 border-[#333]"></div>
        <!-- 极简背景线条 -->
        <div class="absolute inset-y-0 left-1/2 w-px bg-[#111] -translate-x-1/2 pointer-events-none"></div>
        <div class="absolute inset-x-0 top-1/2 h-px bg-[#111] -translate-y-1/2 pointer-events-none"></div>

        <div class="text-center w-full max-w-4xl z-10 select-none">
          <div v-if="currentBookName" class="font-tech text-sm text-amber-500 tracking-widest mb-6 font-bold border border-amber-500/30 px-6 py-2 inline-block bg-amber-500/10 shadow-[0_0_15px_rgba(245,158,11,0.05)]">
            当前书籍: {{ currentBookName }}
          </div>
          
          <div class="relative inline-block border-l-2 border-r-2 border-amber-500/30 bg-[#111] px-10 py-8 mb-6 shadow-2xl min-w-[300px]">
            <!-- 四角刻度 -->
            <div class="absolute top-0 left-0 w-3 h-px bg-amber-500"></div>
            <div class="absolute bottom-0 right-0 w-3 h-px bg-amber-500"></div>
            
            <h2 class="text-2xl md:text-3xl font-bold text-gray-100 break-words leading-snug tracking-wide max-w-2xl px-4">
              {{ currentAudioName === '独立音频轨道待命中' ? '无播放内容' : currentAudioName }}
            </h2>
          </div>
          <div class="text-gray-500 text-[11px] tracking-wider font-tech flex justify-center gap-4 mt-2">
            <span class="border border-[#222] px-3 py-1.5 bg-[#141414] truncate max-w-[400px] flex items-center gap-2 shadow-inner">
              <span class="text-gray-600 font-bold">文件路径:</span> {{ currentAudioPath || '请先导入并选择书籍' }}
            </span>
          </div>
        </div>
      </div>

      <!-- 底部控制总成 -->
      <div class="border-t border-[#1f1f1f] bg-[#0c0c0c] px-6 py-6 flex flex-col justify-center select-none">
        
        <audio 
          ref="audioPlayer" 
          :src="currentAudioUrl" 
          @timeupdate="onTimeUpdate" 
          @loadedmetadata="onLoadedMetadata"
          @ended="playNext"
          @play="isPlaying = true"
          @pause="isPlaying = false"
        ></audio>

        <!-- 自定义鼠标追踪高精度硬核进度条 -->
        <div class="flex items-center gap-4 mb-5">
          <span class="text-[11px] font-tech text-amber-500 w-12 tracking-wide">{{ formatTime(activeBook?.currentTime || currentTime) }}</span>
          <div 
            ref="progressBar"
            class="flex-1 relative group h-4 flex items-center cursor-pointer"
            @mousedown="startSeek"
          >
            <div class="w-full h-[3px] bg-[#222] relative rounded-sm">
              <div 
                class="absolute left-0 top-0 h-full bg-amber-500 transition-none rounded-l-sm"
                :style="{ width: `${duration ? ((activeBook?.currentTime || currentTime) / duration) * 100 : 0}%` }"
              ></div>
              <div 
                class="absolute top-1/2 -mt-1.5 w-1.5 h-3 bg-white opacity-0 group-hover:opacity-100 pointer-events-none shadow-[0_0_5px_#fff]"
                :style="{ left: `calc(${duration ? ((activeBook?.currentTime || currentTime) / duration) * 100 : 0}% - 3px)` }"
              ></div>
            </div>
          </div>
          <span class="text-[11px] font-tech text-gray-500 w-12 text-right tracking-wide">{{ formatTime(duration) }}</span>
        </div>

        <!-- 功能按钮集群 -->
        <div class="flex items-center justify-between">
          
          <!-- 倍速指示器 -->
          <div class="flex border border-[#333] font-tech text-[11px] cursor-pointer shadow-sm">
            <div class="bg-[#141414] px-3 py-2 text-gray-400 border-r border-[#333]">当前倍速</div>
            <div class="px-4 py-2 text-amber-500 hover:bg-amber-500 hover:text-black transition-colors font-bold" @click="setRate(playbackRate === 2.0 ? 0.75 : playbackRate + 0.25)">
              {{ playbackRate.toFixed(2) }}X
            </div>
          </div>

          <!-- 播放执行区块 -->
          <div class="flex items-center justify-center gap-4">
            <button @click="skipTime(-15)" class="px-4 py-2 border border-[#333] text-gray-400 hover:border-amber-500 hover:text-amber-500 transition-colors text-xs font-tech tracking-wider bg-[#111]">
              -15S
            </button>
            <button @click="playPrev" class="px-5 py-2 border border-[#333] text-gray-400 hover:border-amber-500 hover:text-amber-500 transition-colors text-xs font-tech tracking-wider bg-[#111]">
              上一首
            </button>
            <button 
              @click="togglePlay" 
              class="px-12 py-2.5 border border-amber-500 font-bold bg-amber-500 text-black hover:bg-amber-400 transition-all text-sm font-tech tracking-widest shadow-[0_0_15px_rgba(245,158,11,0.15)] active:scale-95"
            >
              <div class="flex items-center gap-2">
                <span v-if="isPlaying" class="w-3 h-3 bg-black"></span>
                <span v-else class="w-0 h-0 border-t-[6px] border-t-transparent border-l-[10px] border-l-black border-b-[6px] border-b-transparent"></span>
                {{ isPlaying ? '暂停' : '播放' }}
              </div>
            </button>
            <button @click="playNext" class="px-5 py-2 border border-[#333] text-gray-400 hover:border-amber-500 hover:text-amber-500 transition-colors text-xs font-tech tracking-wider bg-[#111]">
              下一首
            </button>
            <button @click="skipTime(15)" class="px-4 py-2 border border-[#333] text-gray-400 hover:border-amber-500 hover:text-amber-500 transition-colors text-xs font-tech tracking-wider bg-[#111]">
              +15S
            </button>
          </div>

          <!-- 右侧空置占位，用于保持居中对称 -->
          <div class="flex justify-end gap-1.5 w-24 items-end opacity-20">
             <div class="text-[9px] font-tech text-amber-500 ml-1 leading-none tracking-widest mb-px line-clamp-1">VER 3.0</div>
          </div>

        </div>
      </div>
    </div>

    <!-- 设置面板 -->
    <div v-if="showSettings" class="absolute inset-0 z-50 bg-black/85 flex items-center justify-center backdrop-blur-sm select-none">
      <div class="bg-[#0f0f0f] border border-amber-500/50 p-8 w-[600px] flex flex-col shadow-[0_0_50px_rgba(245,158,11,0.1)]">
        <div class="flex justify-between items-center mb-6 border-b border-[#333] pb-4">
          <h2 class="text-xl text-amber-500 font-bold tracking-widest font-tech">软件设置</h2>
        </div>

        <div class="space-y-5 flex-1 p-1">
          <div class="p-4 border border-[#222] bg-[#141414]">
            <h3 class="text-sm font-bold text-gray-300 mb-3 border-b border-[#333] pb-2">基础设置</h3>
            <label class="flex items-center gap-3 cursor-pointer text-gray-400 hover:text-gray-200">
              <input type="checkbox" v-model="settings.minimizeToTray" class="accent-amber-500 w-4 h-4 cursor-pointer" />
              <span>关闭主面板时: 隐藏到系统托盘 (后台运行常驻)</span>
            </label>
             <label class="flex items-center gap-3 cursor-pointer text-gray-400 hover:text-gray-200 mt-2">
              <input type="checkbox" v-model="settings.enableLocalShortcuts" class="accent-amber-500 w-4 h-4 cursor-pointer" />
              <span>启用应用内快捷键 (空格:播放/暂停, 左右方向键:快进/快退)</span>
            </label>
          </div>

          <div class="p-4 border border-[#222] bg-[#141414]">
            <h3 class="text-sm font-bold text-gray-300 mb-3 border-b border-[#333] pb-2 flex justify-between items-center">
              <span>全局快捷键 (软件在后台时也生效)</span>
              <label class="flex items-center gap-2 text-amber-500 cursor-pointer">
                <input type="checkbox" v-model="settings.enableGlobalShortcuts" class="accent-amber-500 w-4 h-4" />
                启用
              </label>
            </h3>
            <div class="space-y-3" :class="{'opacity-50 pointer-events-none': !settings.enableGlobalShortcuts}">
              <div class="flex justify-between items-center">
                <span class="text-gray-500 text-xs">播放 / 暂停</span>
                <input :value="recordingKeyFor === 'gsPlayPause' ? '按下组合键录制...' : settings.gsPlayPause" @keydown.prevent="e => recordKey(e, 'gsPlayPause')" @focus="recordingKeyFor = 'gsPlayPause'" @blur="recordingKeyFor = ''" readonly class="bg-black cursor-pointer border border-[#333] hover:border-amber-500 focus:border-amber-500 text-amber-500 px-2 py-1 text-xs w-[220px] outline-none text-center transition-colors" />
              </div>
              <div class="flex justify-between items-center">
                <span class="text-gray-500 text-xs">上一首</span>
                <input :value="recordingKeyFor === 'gsPrev' ? '按下组合键录制...' : settings.gsPrev" @keydown.prevent="e => recordKey(e, 'gsPrev')" @focus="recordingKeyFor = 'gsPrev'" @blur="recordingKeyFor = ''" readonly class="bg-black cursor-pointer border border-[#333] hover:border-amber-500 focus:border-amber-500 text-amber-500 px-2 py-1 text-xs w-[220px] outline-none text-center transition-colors" />
              </div>
              <div class="flex justify-between items-center">
                <span class="text-gray-500 text-xs">下一首</span>
                <input :value="recordingKeyFor === 'gsNext' ? '按下组合键录制...' : settings.gsNext" @keydown.prevent="e => recordKey(e, 'gsNext')" @focus="recordingKeyFor = 'gsNext'" @blur="recordingKeyFor = ''" readonly class="bg-black cursor-pointer border border-[#333] hover:border-amber-500 focus:border-amber-500 text-amber-500 px-2 py-1 text-xs w-[220px] outline-none text-center transition-colors" />
              </div>
              <div class="flex justify-between items-center">
                <span class="text-gray-500 text-xs">快退 15 秒</span>
                <input :value="recordingKeyFor === 'gsBackward' ? '按下组合键录制...' : settings.gsBackward" @keydown.prevent="e => recordKey(e, 'gsBackward')" @focus="recordingKeyFor = 'gsBackward'" @blur="recordingKeyFor = ''" readonly class="bg-black cursor-pointer border border-[#333] hover:border-amber-500 focus:border-amber-500 text-amber-500 px-2 py-1 text-xs w-[220px] outline-none text-center transition-colors" />
              </div>
              <div class="flex justify-between items-center">
                <span class="text-gray-500 text-xs">快进 15 秒</span>
                <input :value="recordingKeyFor === 'gsForward' ? '按下组合键录制...' : settings.gsForward" @keydown.prevent="e => recordKey(e, 'gsForward')" @focus="recordingKeyFor = 'gsForward'" @blur="recordingKeyFor = ''" readonly class="bg-black cursor-pointer border border-[#333] hover:border-amber-500 focus:border-amber-500 text-amber-500 px-2 py-1 text-xs w-[220px] outline-none text-center transition-colors" />
              </div>
              <p class="text-xs text-amber-500/80 mt-2 text-center pt-2">点击输入框后，直接按下键盘上的按键组合即可绑定。</p>
            </div>
          </div>
        </div>

        <button @click="showSettings = false; bindGlobalShortcuts()" class="w-full mt-4 py-3 bg-amber-500 text-black font-bold tracking-widest hover:bg-amber-400 focus:outline-none transition-all">
          保存设置
        </button>
      </div>
    </div>
  </div>
</template>