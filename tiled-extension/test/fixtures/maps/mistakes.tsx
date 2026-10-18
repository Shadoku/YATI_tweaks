<?xml version="1.0" encoding="UTF-8"?>
<!-- A tileset with mask properties: a name the enum lacks, and names of an enum the project does
     not have (no GodotNavigationLayers in game.tiled-project). -->
<tileset version="1.10" tiledversion="1.12.0" name="mistake_tiles" tilewidth="16" tileheight="16" tilecount="32" columns="8">
 <properties>
  <property name="collision_layer_0" propertytype="GodotPhysicsLayers" value="World,Nope"/>
  <property name="collision_mask_0" value="1,2-4"/>
  <property name="layers_1" value="Ground"/>
  <property name="note" value="not a mask"/>
 </properties>
 <image source="../importer/tiles.png" width="128" height="64"/>
 <tile id="2" class="Enemy"/>
</tileset>
