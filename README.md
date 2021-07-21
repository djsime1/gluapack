# 📦 gluapack

gluapack is a program that can pack hundreds of Garry's Mod Lua files into just a handful.

## Features

* Quick, easy and portable - perfect for a CI/CD pipeline
* Requires no serverside or clientside binary module
* Speeds up server joining times
* Minimizes the impact of your addons to the Lua file limit
* Supports server, shared and client realms
* 100% compatible with the [`file` library](https://wiki.facepunch.com/gmod/file), [`include`](https://wiki.facepunch.com/gmod/Global.include) and [`CompileFile`](https://wiki.facepunch.com/gmod/Global.CompileFile)
* Works with relative path `include`s

# Example

This is the result of packing 88 files!

![Example](https://user-images.githubusercontent.com/14863743/126511677-86088b25-4896-4606-aa44-621621561dfe.png)

# How does it work?

1. gluapack will locate serverside, clientside and shared files in your addon, according to your [configuration](#Configuration).

2. gluapack will then pack the addon into three parts - serverside, clientside and shared.

3. The clientside and shared packs will be commented out\* and chunked into 64 KiB files.

4. The [gluapack loader](https://github.com/WilliamVenner/gluapack/blob/master/src/gluapack.lua) will be injected into your addon's autorun folder.

5. When the server/client loads/spawns in, the loader will unpack all the packed files into a virtual file system stored in `garrysmod/data/gluapack/vfs/`

6. Any calls to [`file` library](https://wiki.facepunch.com/gmod/file), [`include`](https://wiki.facepunch.com/gmod/Global.include) and [`CompileFile`](https://wiki.facepunch.com/gmod/Global.CompileFile) will additionally use this virtual file system, therefore seamlessly "injecting" your unpacked addon into the game.

\* This is done because on the client the loader reads the clientside/shared chunks from the Lua cache (`garrysmod/cache/lua`). Lua files do not show up in here until they are compiled. Therefore, the entire file is commented out so that compiling the file triggers no Lua errors, and adds the file to the Lua cache so that gluapack can read it.

# Usage

## 👨‍💻 CLI

The program has a simple CLI interface which you can view the help of with:

#### Unix

```bash
./gluapack --help
```

#### Windows

```batch
gluapack.exe --help
```

## 📦 Packing

1. To pack an addon, first (optionally) create a `gluapack.json` file in your addon's root, and [configure gluapack](#configuration) to your needs.

2. Then, simply run the program with the `pack` command and the path to your addon's root (the folder containing `lua/`):

#### Unix

```bash
./gluapack pack "path/to/addon"
```

#### Windows

```batch
gluapack.exe pack "path/to/addon"
```

3. Move `lua/gluapack` (the packed files) and `lua/autorun/*_gluapack_*.lua` (the loader file) into your "production"/packed addon. Make sure to delete any files you have packed from your addons, including entry files. They are no longer needed!

## 📤 Unpacking

To unpack a packed addon, run the program with the `unpack` command and the path to the packed addon:

#### Unix

```bash
./gluapack unpack "path/to/packed-addon"
```

#### Windows

```batch
gluapack.exe unpack "path/to/packed-addon"
```

# Configuration

```js
{
    // The "unique ID" of your addon.
    // This can be any (non-empty) alphanumeric ASCII string.
    // If not specified, a hash of your packed addon will be used instead.
    "unique_id": null,

    // File patterns you want to exclude from being packed.
    "exclude": [],

    // File patterns you want to pack.
    "include_sh": [
        "**/*.sh.lua",
        "**/sh_*.lua",
    ],
    "include_cl": [
        "**/cl_*.lua",
        "**/*.cl.lua",
        "vgui/**/*.lua"
    ],
    "include_sv": [
        "**/sv_*.lua",
        "**/*.sv.lua"
    ],

    // Entry files - these files will be executed immediately after being unpacked.
    "entry_cl": [
        "autorun/client/*.lua",
        "vgui/*.lua"
    ],
    "entry_sh": [
        "autorun/*.lua"
    ],
    "entry_sv": [
        "autorun/server/*.lua"
    ]
}
```

## Limitations

* gluapack requires you to tell it what files should be sent to the client. It performs no analysis on your code to find `AddCSLuaFile` calls.

	* gluapack by default will include common file patterns (such as `lua/**/sh_*.lua`) for networked chunks. See [Configuration](#Configuration) for more information.

* gluapack will cause the client to briefly freeze while spawning into the server to unpack files and build the virtual file system

* gluapack requires you to specify entry file(s) (files that will be executed when the addon is unpacked).

	* gluapack by default will include common file patterns (such as `lua/autorun/**.lua`) as entry files. See [Configuration](#Configuration) for more information.
