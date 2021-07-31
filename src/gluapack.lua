-- Please do not modify this loader or rename it. You may break other addons or the loader itself.
-- If you want to change the hash at the start of the filename, use the "unique_id" config option.
-- If you want to contribute changes to this loader, please do so here:
-- https://github.com/WilliamVenner/gluapack

local function loadEntity(fullPath)
	local dirPath, entType, className = fullPath:match("^(gamemodes/[^/]+/entities/([^/]+)/([^/]+))")
	if not dirPath then
		dirPath, entType, className = fullPath:match("^(([^/]+)/([^/]+))")
		if not dirPath then
			error(("Invalid entity passed to loadEntity! %s"):format(fullPath))
		end
	end

	className = className:gsub("%.lua$", "")

	local init
	while true do
		if dirPath:EndsWith(".lua") then
			init = ("gluapack/vfs/%s.txt"):format(dirPath)
			break
		else
			if SERVER then
				init = ("gluapack/vfs/%s/init.lua.txt"):format(dirPath)
				if file.Exists(init, "DATA") then
					break
				end
			else
				init = ("gluapack/vfs/%s/cl_init.lua.txt"):format(dirPath)
				if file.Exists(init, "DATA") then
					break
				end
			end
			init = ("gluapack/vfs/%s/shared.lua.txt"):format(dirPath)
			if file.Exists(init, "DATA") then
				break
			end
			error(("gluapack failed to load entity %s - no init.lua cl_init.lua or shared.lua found!"):format(fullPath))
		end
	end

	::load::
	if entType == "entities" then
		local f = CompileString(file.Read(init, "DATA"), fullPath)
		if f then
			ENT = { Type = "anim", Base = "base_gmodentity", ClassName = className }
			f()
			scripted_ents.Register(ENT, className)
			ENT = nil
		end
	elseif entType == "weapons" then
		local f = CompileString(file.Read(init, "DATA"), fullPath)
		if f then
			SWEP = { Base = "weapon_base", Primary = {}, Secondary = {}, ClassName = className }
			f()
			weapons.Register(SWEP, className)
			SWEP = nil
		end
	elseif entType == "effects" then
		local f = CompileString(file.Read(init, "DATA"), fullPath)
		if f then
			EFFECT = { ClassName = className }
			f()
			if CLIENT then
				effects.Register(EFFECT, className)
			end
			EFFECT = nil
		end
	else
		error("Unknown entity type - " .. entType)
	end
end

local function includeEntryFiles()
	for _, v in ipairs({ENTRY_FILES_SH}) do
		include(v)
	end

	if SERVER then
		for _, v in ipairs({ENTRY_FILES_SV}) do
			include(v)
		end
	else
		for _, v in ipairs({ENTRY_FILES_CL}) do
			include(v)
		end
	end

	for _, v in ipairs({ENTRY_ENTITIES}) do
		loadEntity(v)
	end
end

if not GLUAPACK_SENTS_LOADED then
	hook.Add("PreRegisterSENT", "gluapack_PreRegisterSENT", function(_, class)
		if class ~= "base_gmodentity" then return end
		GLUAPACK_SENTS_LOADED = true
		hook.Remove("PreRegisterSENT", "gluapack_PreRegisterSENT")
	end)
end

if not GLUAPACK_SWEPS_LOADED then
	hook.Add("PreRegisterSWEP", "gluapack_PreRegisterSWEP", function(_, class)
		if class ~= "weapon_base" then return end
		GLUAPACK_SWEPS_LOADED = true
		hook.Remove("PreRegisterSWEP", "gluapack_PreRegisterSWEP")
	end)
end

local gmsv_gluapack_active = file.Exists("autorun/client/gmsv_gluapack_init.lua", "LUA")

if gluapack_gmod_include and GLUAPACK_SUCCESS then
	if gmsv_gluapack_active then
		if gmsv_gluapack_init then
			includeEntryFiles()
		end
	else
		includeEntryFiles()
	end
	return
end

gluapack_gmod_file_Find = gluapack_gmod_file_Find or file.Find
gluapack_gmod_file_Exists = gluapack_gmod_file_Exists or file.Exists
gluapack_gmod_file_Read = gluapack_gmod_file_Read or file.Read
gluapack_gmod_file_AsyncRead = gluapack_gmod_file_AsyncRead or file.AsyncRead
gluapack_gmod_file_Open = gluapack_gmod_file_Open or file.Open
gluapack_gmod_file_IsDir = SERVER and gluapack_gmod_file_IsDir or file.IsDir or nil
gluapack_gmod_include = gluapack_gmod_include or include
gluapack_gmod_AddCSLuaFile = gluapack_gmod_AddCSLuaFile or AddCSLuaFile
gluapack_gmod_CompileFile = gluapack_gmod_CompileFile or CompileFile
gluapack_gmod_require = gluapack_gmod_require or require

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

	if gluapacks[1] then
		local latest = "autorun/" .. gluapacks[1]

		if latest ~= (debug.getinfo(1, "S").short_src:gsub("^addons/.-/", ""):gsub("^lua/", "")) then
			include(latest)
			return
		end
	end
end

AddCSLuaFile()

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

local TERMINATOR_HACK = string.byte("|")
local clientsideFiles = {}
local chunk
local chunkNetworked = CLIENT and true or nil
local function processChunk(path)
	if path then
		chunk = file_Open(path, "rb", "DATA")
	else
		if not file_Exists("gluapack-temp.dat", "DATA") then
			return
		end
		file.Write("gluapack-temp.dat", util.Decompress(file_Read("gluapack-temp.dat", "DATA")))
		chunk = file_Open("gluapack-temp.dat", "rb", "DATA")
	end

	while not chunk:EndOfFile() do
		local terminator = chunkNetworked and TERMINATOR_HACK or 0

		-- Read path
		local path = {}
		while true do
			local byte = chunk:ReadByte()
			if byte == terminator then
				break
			else
				path[#path + 1] = string.char(byte)
			end
			if chunk:EndOfFile() then
				goto eof
			end
		end

		if chunkNetworked then
			path = table.concat(path)
			clientsideFiles[path] = true
			path = ("gluapack/vfs/%s.txt"):format(path)
		else
			path = ("gluapack/vfs/%s.txt"):format(table.concat(path))
		end

		file.CreateDir((path:gsub("/[^/]-$", "")))

		local remaining
		if chunkNetworked then
			remaining = {}
			while true do
				local byte = chunk:ReadByte()
				if byte == TERMINATOR_HACK then
					break
				else
					remaining[#remaining + 1] = string.char(byte)
				end
				if chunk:EndOfFile() then
					goto eof
				end
			end
			remaining = tonumber(table.concat(remaining), 16)
		else
			remaining = chunk:ReadULong()
		end
		while true do
			local readBytes = math.min(remaining, chunk:Size() - chunk:Tell())
			file.Append(path, chunk:Read(readBytes))
			remaining = remaining - readBytes

			if chunk:EndOfFile() then
				goto eof
			elseif remaining <= 0 then
				assert(remaining == 0)
				break
			end
		end
	end

	::eof::
	chunk:Close()
	chunk = nil
	if not path then
		file.Delete("gluapack-temp.dat")
	end
end

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
	for _, f in ipairs(findSortedChunks(path .. "*.sh.lua")) do
		local path = path .. f
		if SERVER then
			AddCSLuaFile(path)
		end
		file.Append("gluapack-temp.dat", include(path))
	end
	processChunk()

	for _, f in ipairs(findSortedChunks(path .. "*.cl.lua")) do
		local path = path .. f
		if SERVER then
			AddCSLuaFile(path)
		else
			file.Append("gluapack-temp.dat", include(path))
		end
	end
	if CLIENT then
		processChunk()
	else
		local svPath = path .. "gluapack.sv.lua"
		if file_Exists(svPath, "LUA") then
			processChunk(svPath)
		end
	end
end
for _, d in ipairs(select(2, file_Find("gluapack/*", "LUA"))) do
	gluaunpack(("gluapack/%s/"):format(d))
end

local function normalizeMountedPath(path)
	return path:gsub("gamemodes/[^/]+/entities/", ""):gsub("gamemodes/([^/]+)/gamemode", "%1/gamemode")
end

local function findRelativeScript(path)
	local i = 3
	while true do
		local info = debug.getinfo(i, "S")
		if not info then
			break
		end
		local info = normalizeMountedPath(info.short_src):gsub("[^/]+%.lua", path)
		if info == "[C]" then
			return
		end
		local vfsPath = ("gluapack/vfs/%s.txt"):format(info)
		if file_Exists(info, "LUA") then
			return info, false
		elseif file_Exists(vfsPath, "DATA") then
			return vfsPath, true
		end
		i = i + 1
	end
end

if CLIENT then
	-- We have to prevent any scripts from reading the VFS paths - Lua can't read clientside files with file.Read.

	function file.Read(path, gamePath)
		if gamePath and gamePath:lower() == "lua" then
			local vfsPath = ("gluapack/vfs/%s.txt"):format(path)
			if file_Exists(vfsPath, "DATA") and clientsideFiles[path] == nil then
				return file_Read(vfsPath, "DATA")
			end
		end
		return file_Read(path, gamePath)
	end

	function file.AsyncRead(path, gamePath, callback, sync)
		if gamePath and gamePath:lower() == "lua" then
			local vfsPath = ("gluapack/vfs/%s.txt"):format(path)
			if file_Exists(vfsPath, "DATA") and clientsideFiles[path] == nil then
				return file_AsyncRead(vfsPath, "DATA", callback, sync)
			end
		end
		return file_AsyncRead(path, gamePath, callback, sync)
	end
else
	function file.Read(path, gamePath)
		if gamePath and gamePath:lower() == "lua" then
			local vfsPath = ("gluapack/vfs/%s.txt"):format(path)
			if file_Exists(vfsPath, "DATA") then
				return file_Read(vfsPath, "DATA")
			end
		end
		return file_Read(path, gamePath)
	end

	function file.AsyncRead(path, gamePath, callback, sync)
		if gamePath and gamePath:lower() == "lua" then
			local vfsPath = ("gluapack/vfs/%s.txt"):format(path)
			if file_Exists(vfsPath, "DATA") then
				return file_AsyncRead(vfsPath, "DATA", callback, sync)
			end
		end
		return file_AsyncRead(path, gamePath, callback, sync)
	end

	function file.IsDir(path, gamePath)
		if gamePath and gamePath:lower() == "lua" then
			local vfsPath = ("gluapack/vfs/%s"):format(path)
			if gluapack_gmod_file_IsDir(vfsPath, "DATA") then
				return true
			end
		end
		return gluapack_gmod_file_IsDir(path, gamePath)
	end
end

function file.Exists(path, gamePath)
	if gamePath and gamePath:lower() == "lua" and file_Exists(("gluapack/vfs/%s.txt"):format(path), "DATA") then
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
		if gamePath and gamePath:lower() == "lua" then
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
	if (fileMode == "rb" or fileMode == "r") and gamePath and gamePath:lower() == "LUA" then
		local vfsPath = ("gluapack/vfs/%s.txt"):format(fileName)
		if file_Exists(vfsPath, "DATA") then
			return file_Open(vfsPath, fileMode, "DATA")
		end
	end
	return file_Open(fileName, fileMode, gamePath)
end

function _G.require(path)
	local vfsPath = ("gluapack/vfs/includes/modules/%s.lua.txt"):format(path)
	if file_Exists(vfsPath, "DATA") then
		local f = CompileString(file_Read(vfsPath, "DATA"), path)
		if f then
			return f()
		else
			return
		end
	else
		return gluapack_gmod_require(path)
	end
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
		local absolutePath, isVfs = findRelativeScript(path)
		if absolutePath then
			if isVfs then
				local f = CompileString(file_Read(absolutePath, "DATA"), absolutePath:gsub("%.txt$", ""):gsub("^gluapack/vfs/", ""))
				if f then
					return f()
				else
					return
				end
			else
				return include(absolutePath)
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
			local absolutePath, isVfs = findRelativeScript(path)
			if absolutePath then
				if isVfs then
					return
				else
					return AddCSLuaFile(absolutePath)
				end
			else
				return AddCSLuaFile(path)
			end
		end
	end
end

function _G.CompileFile(path, ident)
	local vfsPath = ("gluapack/vfs/%s.txt"):format(path)
	if file_Exists(vfsPath, "DATA") then
		return CompileString(file.Read(vfsPath, "DATA"), ident or path)
	end
	return CompileFile(path, ident)
end

GLUAPACK_SUCCESS = true

if not gmsv_gluapack_active then
	includeEntryFiles()
end