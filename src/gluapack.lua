-- Please do not modify this loader or rename it. You may break other addons or the loader itself.
-- If you want to change the hash at the start of the filename, use the "unique_id" config option.
-- If you want to contribute changes to this loader, please do so here:
-- https://github.com/WilliamVenner/gluapack

local function includeEntryFiles()
	for _, v in ipairs({ENTRY_FILES_SH}) do
		AddCSLuaFile(v)
		include(v)
	end

	if CLIENT then
		for _, v in ipairs({ENTRY_FILES_CL}) do
			AddCSLuaFile(v)
			include(v)
		end
	else
		for _, v in ipairs({ENTRY_FILES_SV}) do
			include(v)
		end
	end
end

if gluapack_gmod_include and GLUAPACK_SUCCESS then return includeEntryFiles() end

gluapack_gmod_file_Find = gluapack_gmod_file_Find or file.Find
gluapack_gmod_file_Exists = gluapack_gmod_file_Exists or file.Exists
gluapack_gmod_file_Read = gluapack_gmod_file_Read or file.Read
gluapack_gmod_file_AsyncRead = gluapack_gmod_file_AsyncRead or file.AsyncRead
gluapack_gmod_file_Open = gluapack_gmod_file_Open or file.Open
gluapack_gmod_include = gluapack_gmod_include or include
gluapack_gmod_AddCSLuaFile = gluapack_gmod_AddCSLuaFile or AddCSLuaFile
gluapack_gmod_CompileFile = gluapack_gmod_CompileFile or CompileFile

local file_Find = gluapack_gmod_file_Find
local file_Exists = gluapack_gmod_file_Exists
local file_Read = gluapack_gmod_file_Read
local file_AsyncRead = gluapack_gmod_file_AsyncRead
local file_Open = gluapack_gmod_file_Open
local include = include
local AddCSLuaFile = AddCSLuaFile
local CompileFile = CompileFile
local CLIENT = CLIENT or false

if GLUAPACK_SUCCESS == nil then
	-- Find the latest gluapack version and execute it (or just continue executing this one, if it's the latest)

	local function compareSemver(a, b)
		local i = 0
		local aIter = (a .. "."):gmatch("(%d+)%.")
		local bIter = (b .. "."):gmatch("(%d+)%.")
		while i < 3 do
			i = i + 1
			local aComponent = tonumber(aIter())
			local bComponent = tonumber(bIter())
			if aComponent < bComponent then
				return true
			elseif aComponent > bComponent then
				return false
			end
		end
		return false
	end

	local gluapacks = (file_Find("autorun/*_gluapack_*.lua", "LUA"))
	table.sort(gluapacks, compareSemver)
	local latest = "autorun/" .. gluapacks[1]

	if latest ~= (debug.getinfo(1, "S").short_src:gsub("^addons/.-/", ""):gsub("^lua/", "")) then
		include(latest)
		return
	end
end

GLUAPACK_SUCCESS = false

do
	local function purge(path)
		local f, d = file_Find(path .. "*", "DATA")
		for _, f in ipairs(f) do
			file.Delete(path .. f)
		end
		for _, d in ipairs(d) do
			purge(("%s%s/"):format(path, d))
			file.Delete(path .. d)
		end
	end
	purge("gluapack/vfs/")
	file.CreateDir("gluapack/vfs")
end

local clientsideFiles = {}
local GLUAPACK_CURRENT_CHUNK
local GLUAPACK_IS_CHUNK_NETWORKED = CLIENT and true or nil
local TERMINATOR_HACK = string.byte("|")
local function processChunk()
	while not GLUAPACK_CURRENT_CHUNK:EndOfFile() do
		local terminator = GLUAPACK_IS_CHUNK_NETWORKED and TERMINATOR_HACK or 0

		-- Read path
		local path = {}
		while true do
			local byte = GLUAPACK_CURRENT_CHUNK:ReadByte()
			if byte == terminator then
				break
			else
				path[#path + 1] = string.char(byte)
			end
			if GLUAPACK_CURRENT_CHUNK:EndOfFile() then
				coroutine.yield()
			end
		end

		if GLUAPACK_IS_CHUNK_NETWORKED then
			path = table.concat(path)
			clientsideFiles[path] = true
			path = ("gluapack/vfs/%s.txt"):format(path)
		else
			path = ("gluapack/vfs/%s.txt"):format(table.concat(path))
		end

		file.CreateDir((path:gsub("/[^/]-$", "")))

		local remaining
		if GLUAPACK_IS_CHUNK_NETWORKED then
			remaining = {}
			while true do
				local byte = GLUAPACK_CURRENT_CHUNK:ReadByte()
				if byte == TERMINATOR_HACK then
					break
				else
					remaining[#remaining + 1] = string.char(byte)
				end
				if GLUAPACK_CURRENT_CHUNK:EndOfFile() then
					coroutine.yield()
				end
			end
			remaining = tonumber(table.concat(remaining), 16)
		else
			remaining = GLUAPACK_CURRENT_CHUNK:ReadULong()
		end
		while true do
			local readBytes = math.min(remaining, GLUAPACK_CURRENT_CHUNK:Size() - GLUAPACK_CURRENT_CHUNK:Tell())
			file.Append(path, GLUAPACK_CURRENT_CHUNK:Read(readBytes))
			remaining = remaining - readBytes

			if GLUAPACK_CURRENT_CHUNK:EndOfFile() then
				coroutine.yield()
			elseif remaining <= 0 then
				assert(remaining == 0)
				break
			end
		end
	end
end

local co, unpackChunk
if CLIENT then
	-- On the client, we read the chunks from the Lua cache.
	-- To get Gmod to add the files to the Lua cache, we need to include/compile the files.
	-- To include/compile the Lua files without creating an error, the files are commented on every new line.
	-- We store their Lua cache file names (SHA1 truncated to 40 bytes) in a manifest file.
	function unpackChunk(path, _, clientCacheManifest)
		local index, realm = path:match("gluapack%.(%d+)%.(.-)%.lua$")
		local cachePath = ("cache/lua/%s.lua"):format(clientCacheManifest[realm][tonumber(index)])

		CompileFile(path) -- This triggers the game to write to cache/lua/<clientCacheManifest[path]>.lua
		assert(file_Exists(cachePath, "GAME"), ("Cached packed file doesn't exist in %s"):format(cachePath))

		local f = file_Open(cachePath, "rb", "GAME")

		-- Skip 32 bytes (SHA256 header)
		f:Skip(32)

		-- Read to EOL
		local chunk = f:Read(f:Size() - 32)
		f:Close()

		-- Decompress
		chunk = util.Decompress(chunk)

		-- Strip comments
		chunk = chunk:sub(3, #chunk - 1):gsub("\n%-%-", "\n")

		-- Write to our temp file "buffer"
		file.Write("gluapack-temp.dat", chunk)
		chunk = nil

		GLUAPACK_CURRENT_CHUNK = file_Open("gluapack-temp.dat", "rb", "DATA")

		-- Continue as normal
		coroutine.resume(co)

		GLUAPACK_CURRENT_CHUNK:Close()
		file.Delete("gluapack-temp.dat")
	end
else
	function unpackChunk(path, isNetworked)
		GLUAPACK_IS_CHUNK_NETWORKED = isNetworked
		if isNetworked then
			-- Is this a shared file? It will have comments...
			local chunk = file_Read(path, "LUA")

			-- Strip comments
			chunk = chunk:sub(3):gsub("\n%-%-", "\n")

			-- Write to our temp file "buffer"
			file.Write("gluapack-temp.dat", chunk)
			chunk = nil
			
			GLUAPACK_CURRENT_CHUNK = file_Open("gluapack-temp.dat", "rb", "DATA")

			-- Continue as normal
			coroutine.resume(co)

			GLUAPACK_CURRENT_CHUNK:Close()
			file.Delete("gluapack-temp.dat")
		else
			-- Otherwise, just read as normal
			GLUAPACK_CURRENT_CHUNK = file_Open(path, "rb", "LUA")
			coroutine.resume(co)
			GLUAPACK_CURRENT_CHUNK:Close()
		end
	end
end

local function resetUnpacker()
	co = coroutine.create(processChunk)
	if GLUAPACK_CURRENT_CHUNK then
		GLUAPACK_CURRENT_CHUNK:Close()
		GLUAPACK_CURRENT_CHUNK = nil
	end
	if SERVER then
		GLUAPACK_IS_CHUNK_NETWORKED = nil
	end
end
resetUnpacker()

local findSortedChunks do
	local function extract(path)
		return tonumber(path:match("gluapack%.(%d+)%..-%.lua$"))
	end
	local function sort(a, b)
		return extract(a) < extract(b)
	end

	function findSortedChunks(path)
		local f = file_Find(path, "LUA")
		table.sort(f, sort)
		return f
	end
end
local function gluaunpack(path)
	local manifestPath, cacheManifest = path .. "manifest.lua"
	if file_Exists(manifestPath, "LUA") then
		if CLIENT then
			cacheManifest = include(manifestPath)
		else
			AddCSLuaFile(manifestPath)
		end
	end

	for _, f in ipairs(findSortedChunks(path .. "*.sh.lua")) do
		local path = path .. f
		if SERVER then
			AddCSLuaFile(path)
		end
		unpackChunk(path, true, cacheManifest)
	end

	resetUnpacker()

	for _, f in ipairs(findSortedChunks(path .. "*.cl.lua")) do
		local path = path .. f
		if SERVER then
			AddCSLuaFile(path)
		end
		unpackChunk(path, true, cacheManifest)
	end

	resetUnpacker()

	if SERVER then
		local svPath = path .. "gluapack.sv.lua"
		if file_Exists(svPath, "LUA") then
			unpackChunk(svPath)
			resetUnpacker()
		end
	end
end
for _, d in ipairs(select(2, file_Find("gluapack/*", "LUA"))) do
	gluaunpack(("gluapack/%s/"):format(d))
end

local function getRelativeDir(path)
	return (debug.getinfo(3, "S").short_src:gsub("/[^/]+$", ""))
end

if CLIENT then
	-- We have to prevent any scripts from reading the VFS paths - Lua can't read clientside files with file.Read.

	function file.Read(path, gamePath)
		if gamePath:lower() == "lua" then
			local vfsPath = ("gluapack/vfs/%s.txt"):format(path)
			if file_Exists(vfsPath, "DATA") and clientsideFiles[path] == nil then
				return file_Read(vfsPath, "DATA")
			end
		end
		return file_Read(path, gamePath)
	end

	function file.AsyncRead(path, gamePath, callback, sync)
		if gamePath:lower() == "lua" then
			local vfsPath = ("gluapack/vfs/%s.txt"):format(path)
			if file_Exists(vfsPath, "DATA") and clientsideFiles[path] == nil then
				return file_AsyncRead(vfsPath, "DATA", callback, sync)
			end
		end
		return file_AsyncRead(path, gamePath, callback, sync)
	end
else
	function file.Read(path, gamePath)
		if gamePath:lower() == "lua" then
			local vfsPath = ("gluapack/vfs/%s.txt"):format(path)
			if file_Exists(vfsPath, "DATA") then
				return file_Read(vfsPath, "DATA")
			end
		end
		return file_Read(path, gamePath)
	end

	function file.AsyncRead(path, gamePath, callback, sync)
		if gamePath:lower() == "lua" then
			local vfsPath = ("gluapack/vfs/%s.txt"):format(path)
			if file_Exists(vfsPath, "DATA") then
				return file_AsyncRead(vfsPath, "DATA", callback, sync)
			end
		end
		return file_AsyncRead(path, gamePath, callback, sync)
	end
end

function file.Exists(path, gamePath)
	if gamePath:lower() == "lua" and file_Exists(("gluapack/vfs/%s.txt"):format(path), "DATA") then
		return true
	end
	return file_Exists(path, gamePath)
end

do
	local fileDates
	local function sortDatesAsc(a, b)
		return fileDates[a] < fileDates[b]
	end
	local function sortDatesDesc(a, b)
		return fileDates[a] > fileDates[b]
	end
	local function sortNamesDesc(a, b)
		return a > b
	end
	function file.Find(path, gamePath, _sorting)
		if gamePath:lower() == "lua" then
			-- bleh
			local pattern = path:gsub(".lua", ".lua.txt")
			local sorting = _sorting or "nameasc"
			local isDateSort = sorting:StartWith("date")

			local f, d = file_Find(path, gamePath, _sorting)

			if isDateSort then
				local parentDir = pattern:gsub("[^/]+$", "") .. "/"
				do
					local vfsF, vfsD = file_Find(("gluapack/vfs/%s"):format(parentDir), "DATA", _sorting)
					if #vfsF == 0 and #vfsD == 0 then
						return f, d
					else
						fileDates = {}
					end
					for _, vfsF in ipairs(vfsF) do
						f[#f + 1] = vfsF:sub(1, -5)
						fileDates[f] = file.Time(("gluapack/vfs/%s%s"):format(parentDir, vfsF), "DATA")
					end
					for _, vfsD in ipairs(vfsD) do
						d[#d + 1] = vfsD
						fileDates[d] = file.Time(("gluapack/vfs/%s%s"):format(parentDir, vfsD), "DATA")
					end
				end
				for _, f in ipairs(f) do
					fileDates[f] = file.Time(("%s%s"):format(parentDir, f), gamePath)
				end
				for _, f in ipairs(d) do
					fileDates[d] = file.Time(("%s%s"):format(parentDir, d), gamePath)
				end
			else
				local vfsF, vfsD = file_Find(("gluapack/vfs/%s"):format(pattern), "DATA", _sorting)
				if #vfsF == 0 and #vfsD == 0 then
					return f, d
				end
				for i = 1, #vfsF do
					f[#f + 1] = vfsF[i]:sub(1, -5)
				end
				for i = 1, #vfsD do
					d[#d + 1] = vfsD[i]
				end
			end

			local sortingFunc
			if isDateSort then
				if sorting == "dateasc" then
					sortingFunc = sortDatesAsc
				elseif sorting == "datedesc" then
					sortingFunc = sortDatesDesc
				end
			elseif sorting == "namedesc" then
				sortingFunc = sortNamesDesc
			end

			table.sort(f, sortingFunc)
			table.sort(d, sortingFunc)

			fileDates = nil

			return f, d
		end

		return file_Find(path, gamePath, sorting)
	end
end

function file.Open(fileName, fileMode, gamePath)
	if (fileMode == "rb" or fileMode == "r") and gamePath:lower() == "LUA" then
		local vfsPath = ("gluapack/vfs/%s.txt"):format(fileName)
		if file_Exists(vfsPath, "DATA") then
			return file_Open(vfsPath, fileMode, "DATA")
		end
	end
	return file_Open(fileName, fileMode, gamePath)
end

function _G.include(path)
	local vfsPath = ("gluapack/vfs/%s.txt"):format(path)
	if file_Exists(vfsPath, "DATA") then
		local f = CompileString(file_Read(vfsPath, "DATA"), path)
		if f then
			return f()
		else
			return
		end
	elseif file_Exists(path, "LUA") then
		-- Saves us from resolving the relative path
		return include(path)
	else
		vfsPath = ("gluapack/vfs/%s/%s.txt"):format(getRelativeDir(path), path)
		if file_Exists(vfsPath, "DATA") then
			local f = CompileString(file_Read(vfsPath, "DATA"), path)()
			if f then
				return f()
			else
				return
			end
		else
			return include(path)
		end
	end
end

if SERVER then
	function _G.AddCSLuaFile(path)
		if path == nil then
			return AddCSLuaFile((debug.getinfo(2, "S").short_src:gsub("^lua/", "")))
		end

		-- This function intentionally does nothing for VFS files, since we've already AddCSLuaFile'd them.
		local vfsPath = ("gluapack/vfs/%s.txt"):format(path)
		if file_Exists(vfsPath, "DATA") then
			return
		elseif file_Exists(path, "LUA") then
			-- Saves us from resolving the relative path
			return AddCSLuaFile(path)
		else
			vfsPath = ("gluapack/vfs/%s/%s.txt"):format(getRelativeDir(path), path)
			if file_Exists(vfsPath, "DATA") then
				return
			else
				return AddCSLuaFile(path)
			end
		end
	end
end

function _G.CompileFile(path)
	local vfsPath = ("gluapack/vfs/%s.txt"):format(path)
	if file_Exists(vfsPath, "DATA") then
		return CompileString(file.Read(vfsPath, "DATA"), path)
	end
	return CompileFile(path)
end

GLUAPACK_SUCCESS = true

includeEntryFiles()