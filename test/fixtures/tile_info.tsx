<?xml version="1.0" encoding="UTF-8"?>
<!-- info.tile of tile objects (tile_info.tmx): a shared tileset whose bush tile has a class, custom
     properties and collision shapes (a rectangle, a polygon, a turned ellipse), and an animated tile. -->
<tileset version="1.10" tiledversion="1.12.0" name="props" tilewidth="16" tileheight="16" tilecount="32" columns="8">
 <image source="tiles.png" width="128" height="64"/>
 <tile id="5" class="Bush">
  <properties>
   <property name="hp" type="int" value="3"/>
   <property name="slashable" type="bool" value="true"/>
   <property name="sound" value="rustle"/>
  </properties>
  <objectgroup draworder="index" id="2">
   <object id="1" name="Trunk" x="2" y="4" width="12" height="10"/>
   <object id="2" name="Leaf" x="8" y="0">
    <polygon points="0,0 4,6 -4,6"/>
   </object>
   <object id="3" name="Round" x="0" y="8" width="16" height="8" rotation="15">
    <ellipse/>
   </object>
  </objectgroup>
 </tile>
 <tile id="9" class="Bush">
  <properties>
   <property name="sound" value="crackle"/>
  </properties>
  <animation>
   <frame tileid="9" duration="100"/>
   <frame tileid="10" duration="250"/>
  </animation>
 </tile>
</tileset>
