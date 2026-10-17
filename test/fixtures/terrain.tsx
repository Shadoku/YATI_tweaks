<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.11.2" name="terrain" tilewidth="16" tileheight="16" tilecount="32" columns="8">
 <properties>
  <property name="author" value="yati-tests"/>
 </properties>
 <image source="tiles.png" width="128" height="64"/>
 <!-- tile 0: custom data (data_ prefix) and plain metadata -->
 <tile id="0">
  <properties>
   <property name="data_speed" type="int" value="3"/>
   <property name="data_slippery" type="bool" value="true"/>
   <property name="kind" value="grass"/>
  </properties>
 </tile>
 <!-- tile 3: probability -->
 <tile id="3" probability="0.25"/>
 <!-- tile 4: animation over tiles 4,5,6 (in a row). Tiles 5 and 6 must never be placed. -->
 <tile id="4">
  <animation>
   <frame tileid="4" duration="100"/>
   <frame tileid="5" duration="100"/>
   <frame tileid="6" duration="100"/>
  </animation>
 </tile>
 <!-- tile 8: collision shapes -->
 <tile id="8">
  <objectgroup draworder="index" id="2">
   <object id="1" name="CRect" x="0" y="0" width="16" height="8"/>
   <object id="2" name="CEllipse" x="2" y="8" width="8" height="6">
    <ellipse/>
   </object>
   <object id="3" name="CPoly" x="8" y="8">
    <polygon points="0,0 8,0 8,8"/>
   </object>
   <object id="4" name="COneWay" x="0" y="12">
    <properties>
     <property name="one_way" type="bool" value="true"/>
    </properties>
    <polygon points="0,0 16,0 16,4"/>
   </object>
   <object id="5" name="CLayer1" x="4" y="4" width="8" height="8">
    <properties>
     <property name="physics_layer" type="int" value="1"/>
    </properties>
   </object>
  </objectgroup>
 </tile>
 <!-- tile 10: has three properties (used by tile objects) -->
 <tile id="10">
  <properties>
   <property name="loot" value="gold"/>
   <property name="weight" type="float" value="1.5"/>
   <property name="stack" type="int" value="4"/>
  </properties>
 </tile>
 <wangsets>
  <wangset name="Ground" type="corner" tile="-1">
   <wangcolor name="Grass" color="#00ff00" tile="-1" probability="1"/>
   <wangcolor name="Dirt" color="#a05000" tile="-1" probability="1"/>
   <wangtile tileid="16" wangid="0,1,0,1,0,1,0,1"/>
   <wangtile tileid="17" wangid="0,2,0,2,0,2,0,2"/>
   <wangtile tileid="18" wangid="0,1,0,2,0,2,0,1"/>
   <wangtile tileid="19" wangid="0,2,0,1,0,1,0,2"/>
  </wangset>
 </wangsets>
</tileset>
