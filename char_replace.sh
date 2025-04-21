#!/bin/bash
#!/usr/bin/bash  # 如果 which bash 返回 /usr/bin/bash

# 定义要替换的字符映射（中文全角 → 英文半角）
declare -A replace_map=(
    ['（']='('
    ['）']=')'
    ['，']=','
    ['。']='.'
    ['；']=';'
    ['：']=':'
)

# 初始化计数器
declare -A count_stats=(
    ['（ → (' ]=0
    ['） → )' ]=0
    ['， → ,' ]=0
    ['。 → .' ]=0
    ['； → ;' ]=0
    ['： → :' ]=0
)

# 获取当前脚本名称
current_script_name=$(basename "$0")

# 递归查找所有文本文件(排除二进制文件和指定文件)
while IFS= read -r -d '' file; do
    # 获取文件名（不含路径）
    filename=$(basename "$file")
    
    # 检查是否为需要排除的文件
    if [[ "$filename" == "$current_script_name" || "$filename" == "char_replace.ps1" ]]; then
        echo "跳过排除文件: $file"
        continue
    fi

    # 检查是否为文本文件(避免修改二进制文件)
    if file "$file" | grep -q 'text'; then
        echo "处理文件: $file"
        temp_file=$(mktemp)
        cp "$file" "$temp_file"

        # 逐个替换字符并统计次数
        for key in "${!replace_map[@]}"; do
            value="${replace_map[$key]}"
            # 统计替换前的出现次数
            old_count=$(grep -o "$key" "$temp_file" | wc -l)
            # 执行替换
            sed -i "s/$key/$value/g" "$temp_file"
            # 统计替换后的出现次数
            new_count=$(grep -o "$key" "$temp_file" | wc -l)
            # 计算实际替换次数
            replaced_count=$((old_count - new_count))
            # 更新统计
            count_stats["$key → $value"]=$((count_stats["$key → $value"] + replaced_count))
        done

        # 替换原文件
        mv "$temp_file" "$file"
    else
        echo "跳过非文本文件: $file"
    fi
done < <(find . -type f -print0)

# 输出替换统计
echo -e "\n替换统计："
for key in "${!count_stats[@]}"; do
    echo "$key: ${count_stats[$key]} 次"
done

echo -e "\n所有文件处理完成！"