<div align="center">
  <img src="docs/banner.png" alt="Synapse Banner" width="100%" />

  <h1>⚡ Synapse</h1>
  <p><strong>Next-Gen Windows Automation Engine</strong></p>
  <p>
    <em>Kullanıcı dostu görsel editör • Yüksek performanslı Rust çekirdeği • GPU-hızlandırmalı görüntü işleme</em>
  </p>

  <br />

  <p>
    <img src="https://img.shields.io/badge/Tauri-2.0-blue?logo=tauri&logoColor=white" alt="Tauri 2" />
    <img src="https://img.shields.io/badge/Rust-2021-orange?logo=rust&logoColor=white" alt="Rust" />
    <img src="https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=white" alt="React" />
    <img src="https://img.shields.io/badge/TypeScript-5.6-3178C6?logo=typescript&logoColor=white" alt="TypeScript" />
    <img src="https://img.shields.io/badge/License-MIT-green" alt="License" />
    <img src="https://img.shields.io/badge/Platform-Windows-0078D4?logo=windows&logoColor=white" alt="Platform" />
  </p>
</div>

---

## 🎯 Vizyon

Synapse, piyasadaki otomasyon araçlarının **"ya çok basit ve yeteneksiz ya da çok güçlü ama kullanımı çok zor"** ikilemini ortadan kaldıran bir köprü projesidir.

Hiç programlama bilmeyen biri bile, **sürükle-bırak** mantığıyla bilgisayardaki en karmaşık işlemleri otomatiğe bağlayabilir. Oyun motorlarındaki görsel programlama felsefesini, gündelik masaüstü otomasyonuna taşıyoruz.

<div align="center">
  <img src="docs/screenshot.png" alt="Synapse UI" width="90%" />
  <br/>
  <em>Düğüm tabanlı görsel editör — Tetikleyiciler, Eylemler ve Koşullar</em>
</div>

## ✨ Özellikler

### 🎨 Görsel Editör
- **Düğüm tabanlı** sürükle-bırak arayüz (React Flow)
- **16 düğüm tipi** — tetikleyiciler, eylemler, koşullar, döngüler
- **Gerçek zamanlı** düğüm durumu geri bildirimi (aktif düğüm yeşil parlar)
- **Özellik paneli** ile düğüm bazlı konfigürasyon
- Koyu tema, micro-animasyonlar, premium tasarım

### 🦀 Rust Çekirdeği
- **JSON → IR Derleyici**: Görsel graph'ı topolojik sıralı ara temsile (IR) çevirir
- **Yüksek performanslı executor**: Dedicated `std::thread` ile `THREAD_PRIORITY_ABOVE_NORMAL`
- **Tokio-free input timing**: Milisaniyelik hassasiyet, sıfır jitter
- **Tauri Event**: Gerçek zamanlı UI ↔ Backend iletişimi

### 🖱️ Giriş Simülasyonu
- **Fare**: Tıklama, çift tıklama, basılı tutma, insansı hareket (Bézier / Humanized eğriler)
- **Klavye**: Tuş vuruşu, modifiye tuşlar (Ctrl/Alt/Shift/Win), metin yazma
- **Jitter**: Rastgele gecikme ile insan benzeri davranış
- **Katmanlı mimari**: L1 (SendInput) → L2 (Interception) → L3 (Virtual HID) [roadmap]

### 👁️ Görüntü İşleme
- **GDI BitBlt** ile hızlı ekran yakalama
- **Piksel renk analizi**: Toleranslı renk eşleme ve arama
- **Template matching**: Normalized Cross-Correlation (NCC) ile görsel arama
- **BMP desteği**: 24-bit ve 32-bit template yükleme

### 💾 Akış Yönetimi
- Flow'ları JSON olarak kaydet / yükle / sil
- `%APPDATA%/Synapse/flows/` dizininde kalıcı depolama
- Değişken sistemi (runtime context)

## 🏗️ Mimari

```
┌─────────────────────────────────────────────────────────┐
│                    React Frontend                        │
│  ┌──────────┐ ┌──────────┐ ┌───────────┐ ┌───────────┐  │
│  │ Sidebar  │ │  Canvas  │ │Properties │ │  Toolbar   │  │
│  │ (Palette)│ │(ReactFlow)│ │  Panel    │ │(Controls) │  │
│  └──────────┘ └──────────┘ └───────────┘ └───────────┘  │
│            Zustand Store (flowStore / executionStore)     │
├──────────────────── Tauri IPC ──────────────────────────┤
│                     Rust Backend                         │
│  ┌──────────┐ ┌──────────┐ ┌───────────┐ ┌───────────┐  │
│  │ Compiler │ │ Executor │ │   Input   │ │  Vision   │  │
│  │(JSON→IR) │ │(IR→Run)  │ │(enigo/L1) │ │(GDI/NCC)  │  │
│  └──────────┘ └──────────┘ └───────────┘ └───────────┘  │
│              Storage (fs) │ Context (vars/loops)         │
└─────────────────────────────────────────────────────────┘
```

## 🚀 Başlangıç

### Gereksinimler

| Araç | Versiyon |
|------|----------|
| [Node.js](https://nodejs.org/) | ≥ 18.x |
| [Rust](https://rustup.rs/) | ≥ 1.75 |
| [VS Build Tools](https://visualstudio.microsoft.com/downloads/) | 2022+ (C++ Desktop workload) |

### Kurulum

```bash
# Depoyu klonla
git clone https://github.com/cekYc/Synapse.git
cd Synapse

# Frontend bağımlılıkları
npm install

# Geliştirme sunucusu (Tauri + Vite)
npm run tauri dev
```

> **Not**: İlk derleme ~400 Rust crate indirip derleyecektir. Bu işlem 2-5 dakika sürebilir.

### Derleme (Production)

```bash
npm run tauri build
```

Çıktı: `src-tauri/target/release/synapse.exe`

## 📁 Proje Yapısı

```
synapse/
├── src/                          # Frontend (React + TypeScript)
│   ├── components/
│   │   ├── layout/               # Toolbar, Sidebar, StatusBar, PropertiesPanel
│   │   └── nodes/                # NodeWrapper (custom React Flow nodes)
│   ├── stores/                   # Zustand state (flowStore, executionStore)
│   ├── types/                    # TypeScript tip tanımları
│   ├── utils/                    # nodeRegistry (16 düğüm tipi)
│   ├── App.tsx                   # Ana uygulama
│   └── index.css                 # Tasarım sistemi (60+ CSS token)
│
├── src-tauri/                    # Backend (Rust)
│   └── src/
│       ├── engine/               # Çekirdek motor
│       │   ├── ir.rs             # Intermediate Representation (13 opcode)
│       │   ├── compiler.rs       # JSON graph → IR derleyici
│       │   ├── executor.rs       # IR yürütücü (dedicated thread)
│       │   └── context.rs        # Runtime context (vars, loops, pause)
│       ├── input/                # Giriş simülasyonu
│       │   ├── mod.rs            # InputBackend trait
│       │   └── standard.rs       # L1: enigo/SendInput backend
│       ├── vision/               # Görüntü işleme
│       │   ├── capture.rs        # GDI BitBlt ekran yakalama
│       │   ├── pixel.rs          # Piksel renk analizi
│       │   └── template.rs       # NCC template matching
│       ├── commands/             # Tauri IPC komutları
│       ├── storage/              # Dosya sistemi depolama
│       └── lib.rs                # Modül kökü
│
├── docs/                         # Dokümantasyon ve görseller
└── package.json
```

## 🧩 Düğüm Tipleri

| Kategori | Düğüm | Açıklama |
|----------|--------|----------|
| 🔴 **Tetikleyiciler** | Kısayol Tuşu | Tuş kombinasyonuyla başlat |
| | Piksel Rengi | Belirli renk ekranda göründüğünde |
| | Görsel Eşleme | Template eşleştiğinde |
| | Zamanlayıcı | Belirli aralıklarla tekrarla |
| 🔵 **Eylemler** | Fare Tıklama | Sol/sağ/orta tıklama |
| | Fare Hareketi | İnsansı hareket eğrileriyle |
| | Tuş Vuruşu | Modifier + tuş basma |
| | Metin Yaz | Karakter karakter yazma |
| | Bekle | Konfigüre edilebilir gecikme |
| | Program Çalıştır | Harici uygulama başlat |
| | Değişken Ata | Runtime değişken güncelle |
| 🟡 **Koşullar** | Koşul (Eğer) | if/else dallanma |
| | Piksel Kontrolü | Piksel renk doğrulama |
| | Görsel Var mı? | Template varlık kontrolü |
| 🟢 **Akış** | Döngü | Sayaçlı tekrar |
| | Koşullu Döngü | While döngüsü |

## 🗺️ Yol Haritası

- [x] **Faz 1**: Temel İskelet — Tauri 2, React Flow, tasarım sistemi
- [x] **Faz 2**: Çekirdek Motor — IR, derleyici, executor, giriş simülasyonu
- [x] **Faz 3**: Görüntü İşleme — Ekran yakalama, piksel analizi, template matching
- [ ] **Faz 4**: Gelişmiş Giriş — Interception driver (L2), anti-detection
- [ ] **Faz 5**: Performans — DXGI Desktop Duplication, wgpu compute shader
- [ ] **Faz 6**: Eklenti Sistemi — Lua/JS scripting desteği
- [ ] **Faz 7**: Topluluk — Flow paylaşım platformu

## 🤝 Katkıda Bulunma

Katkılarınız memnuniyetle karşılanır! Lütfen:

1. Projeyi fork edin
2. Feature branch oluşturun (`git checkout -b feature/amazing-feature`)
3. Değişikliklerinizi commit edin (`git commit -m 'feat: add amazing feature'`)
4. Branch'i push edin (`git push origin feature/amazing-feature`)
5. Pull Request açın

## 📄 Lisans

Bu proje [MIT Lisansı](LICENSE) altında lisanslanmıştır.

## 🙏 Teşekkürler

- [Tauri](https://tauri.app/) — Hafif, güvenli masaüstü uygulama framework'ü
- [React Flow](https://reactflow.dev/) — Güçlü düğüm tabanlı editör kütüphanesi
- [enigo](https://github.com/enigo-rs/enigo) — Cross-platform giriş simülasyonu
- [Zustand](https://zustand-demo.pmnd.rs/) — Minimal, esnek state yönetimi

---

<div align="center">
  <sub>Built with 🦀 Rust + ⚛️ React by <a href="https://github.com/cekYc">cekYc</a></sub>
</div>
