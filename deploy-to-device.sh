TARGET=$(ls -d /run/user/$UID/gvfs/mtp* | head -1)/Card\ Storage
(
  cd "$TARGET"
  mkdir demo
  mv -v *.json *.jpg *.JPG *.png *.m3u demo
)

cp --no-preserve=all -vr family "$TARGET/"
cp --no-preserve=all -vr family.m3u "$TARGET/"

