// Every member of Tiled's scripting API the extension uses, with the page of the Tiled 1.12
// reference (https://www.mapeditor.org/docs/scripting/, generated from @mapeditor/tiled-api
// 1.12.0's index.d.ts) that documents it. The fake (fake_tiled.mjs) implements exactly these;
// api_surface.test.mjs fails when the glue touches anything else, or stops using one of these.
//
// A member is keyed by the class that documents it: Project.property is TiledObject's.

export const REFERENCE = "https://www.mapeditor.org/docs/scripting/";

export const API_SURFACE = {
	// The tiled module (modules/tiled.html)
	"tiled.registerAction": "modules/tiled.html#registerAction",
	"tiled.extendMenu": "modules/tiled.html#extendMenu",
	"tiled.assetAboutToBeSaved": "modules/tiled.html#assetAboutToBeSaved",
	"tiled.project": "modules/tiled.html#project (since 1.10.1; null without a project: src/tiled/projectmodel.cpp's editableProject())",
	"tiled.activeAsset": "modules/tiled.html#activeAsset",
	"tiled.openAssets": "modules/tiled.html#openAssets",
	"tiled.open": "modules/tiled.html#open",
	"tiled.mapEditor": "modules/tiled.html#mapEditor",
	"tiled.alert": "modules/tiled.html#alert",
	"tiled.confirm": "modules/tiled.html#confirm",
	"tiled.prompt": "modules/tiled.html#prompt",
	"tiled.promptDirectory": "modules/tiled.html#promptDirectory (since 1.10.2)",
	"tiled.log": "modules/tiled.html#log",
	"tiled.warn": "modules/tiled.html#warn",
	"tiled.error": "modules/tiled.html#error",
	"tiled.color": "modules/tiled.html#color (since 1.11)",
	"tiled.filePath": "modules/tiled.html#filePath",
	"tiled.objectRef": "modules/tiled.html#objectRef",
	"tiled.propertyValue": "modules/tiled.html#propertyValue (since 1.8)",

	// Signal, Action (interfaces/Signal.html, interfaces/Action.html)
	"Signal.connect": "interfaces/Signal.html#connect",
	"Action.text": "interfaces/Action.html#text",

	// Project (classes/Project.html, since 1.10.1; the type API since 1.12)
	"Project.fileName": "classes/Project.html#fileName",
	"Project.propertyTypes": "classes/Project.html#propertyTypes (since 1.12)",
	"Project.findTypeByName": "classes/Project.html#findTypeByName (since 1.12)",
	"Project.addClassType": "classes/Project.html#addClassType (since 1.12)",
	"Project.addEnumType": "classes/Project.html#addEnumType (since 1.12)",

	// TiledObject (classes/TiledObject.html), the base of Project, assets, layers, objects, tiles
	"TiledObject.property": "classes/TiledObject.html#property",
	"TiledObject.setProperty": "classes/TiledObject.html#setProperty",
	"TiledObject.properties": "classes/TiledObject.html#properties",
	"TiledObject.resolvedProperties": "classes/TiledObject.html#resolvedProperties",
	"TiledObject.className": "classes/TiledObject.html#className (since 1.9)",

	// PropertyType, ClassPropertyType, EnumPropertyType (classes/*.html, since 1.12)
	"PropertyType.name": "classes/PropertyType.html#name",
	"PropertyType.isClass": "classes/PropertyType.html#isClass",
	"PropertyType.isEnum": "classes/PropertyType.html#isEnum",
	"ClassPropertyType.members": "classes/ClassPropertyType.html#members",
	"ClassPropertyType.setMember": "classes/ClassPropertyType.html#setMember",
	"ClassPropertyType.removeMember": "classes/ClassPropertyType.html#removeMember",
	"ClassPropertyType.color": "classes/ClassPropertyType.html#color",
	"ClassPropertyType.drawFill": "classes/ClassPropertyType.html#drawFill",
	"ClassPropertyType.usageFlags": "classes/ClassPropertyType.html#usageFlags",
	"ClassPropertyType.PropertyValueType": "classes/ClassPropertyType.html#PropertyValueType",
	"ClassPropertyType.MapObjectClass": "classes/ClassPropertyType.html#MapObjectClass",
	"ClassPropertyType.TileClass": "classes/ClassPropertyType.html#TileClass",
	"EnumPropertyType.storageType": "classes/EnumPropertyType.html#storageType",
	"EnumPropertyType.values": "classes/EnumPropertyType.html#values",
	"EnumPropertyType.nameOf": "classes/EnumPropertyType.html#nameOf",
	"StorageType.StringValue": "enums/StorageType.html#StringValue (since 1.12)",
	"StorageType.IntValue": "enums/StorageType.html#IntValue (since 1.12)",

	// Assets (classes/Asset.html, classes/TileMap.html, classes/Tileset.html)
	"Asset.fileName": "classes/Asset.html#fileName",
	"Asset.isTileMap": "classes/Asset.html#isTileMap",
	"Asset.isTileset": "classes/Asset.html#isTileset",
	"Asset.modified": "classes/Asset.html#modified",
	"Asset.save": "classes/Asset.html#save",
	"TileMap.layers": "classes/TileMap.html#layers",
	"TileMap.selectedObjects": "classes/TileMap.html#selectedObjects",
	"TileMap.currentLayer": "classes/TileMap.html#currentLayer",
	"TileMap.pixelToScreen": "classes/TileMap.html#pixelToScreen",
	"Tileset.name": "classes/Tileset.html#name",

	// Layers and objects (classes/Layer.html, GroupLayer.html, ObjectGroup.html, MapObject.html)
	"Layer.id": "classes/Layer.html#id",
	"Layer.name": "classes/Layer.html#name",
	"Layer.isObjectLayer": "classes/Layer.html#isObjectLayer",
	"Layer.isGroupLayer": "classes/Layer.html#isGroupLayer",
	"GroupLayer.layers": "classes/GroupLayer.html#layers",
	"ObjectGroup.objects": "classes/ObjectGroup.html#objects",
	"MapObject.id": "classes/MapObject.html#id",
	"MapObject.name": "classes/MapObject.html#name",
	"MapObject.x": "classes/MapObject.html#x",
	"MapObject.y": "classes/MapObject.html#y",
	"MapObject.tile": "classes/MapObject.html#tile",

	// The map editor (interfaces/MapEditor.html, interfaces/MapView.html)
	"MapEditor.currentMapView": "interfaces/MapEditor.html#currentMapView",
	"MapView.centerOn": "interfaces/MapView.html#centerOn",

	// Files and processes (classes/TextFile.html, modules/File.html, classes/Process.html)
	"TextFile.constructor": "classes/TextFile.html#constructor",
	"TextFile.ReadOnly": "classes/TextFile.html#ReadOnly",
	"TextFile.WriteOnly": "classes/TextFile.html#WriteOnly",
	"TextFile.readAll": "classes/TextFile.html#readAll",
	"TextFile.write": "classes/TextFile.html#write",
	"TextFile.commit": "classes/TextFile.html#commit",
	"TextFile.close": "classes/TextFile.html#close",
	"File.exists": "modules/File.html#exists",
	"File.directoryEntries": "modules/File.html#directoryEntries",
	"File.Dirs": "modules/File.html#Dirs",
	"File.Files": "modules/File.html#Files",
	"File.Hidden": "modules/File.html#Hidden",
	"File.NoDotAndDotDot": "modules/File.html#NoDotAndDotDot",
	"Process.constructor": "classes/Process.html#constructor (since 1.5)",
	"Process.exec": "classes/Process.html#exec",
	"Process.readStdOut": "classes/Process.html#readStdOut",
	"Process.readStdErr": "classes/Process.html#readStdErr",
	"Process.close": "classes/Process.html#close",
};

// The documenting classes of each fake class, most specific first.
export const CLASS_CHAINS = {
	tiled: ["tiled"],
	Signal: ["Signal"],
	Action: ["Action"],
	Project: ["Project", "TiledObject"],
	ClassPropertyType: ["ClassPropertyType", "PropertyType"],
	EnumPropertyType: ["EnumPropertyType", "PropertyType"],
	StorageType: ["StorageType"],
	TileMap: ["TileMap", "Asset", "TiledObject"],
	Tileset: ["Tileset", "Asset", "TiledObject"],
	ObjectGroup: ["ObjectGroup", "Layer", "TiledObject"],
	GroupLayer: ["GroupLayer", "Layer", "TiledObject"],
	TileLayer: ["TileLayer", "Layer", "TiledObject"],
	MapObject: ["MapObject", "TiledObject"],
	Tile: ["Tile", "TiledObject"],
	MapEditor: ["MapEditor"],
	MapView: ["MapView"],
	TextFile: ["TextFile"],
	File: ["File"],
	Process: ["Process"],
};
