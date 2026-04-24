uniform sampler2D s0;
uniform sampler2D s1;

static const float4 c13 = float4(0.0005, 1.0, 0.5, -2.0);

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t1 : TEXCOORD1;
    float4 t2 : TEXCOORD2;
    float4 t3 : TEXCOORD3;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r1 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r2 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r3 = float4(0.0, 0.0, 0.0, 0.0);

    r0.w = (1.0 / input.t3.w);
    r0.x = (input.t3.x * r0.w + c13.y);
    r0.y = (input.t3.y * r0.w + c13.y);
    r0.x = (r0.x * c6.z);
    r0.z = c13.z;
    r0.z = (r0.x * r0.z + c6.x);
    r0.x = (r0.y * -(c13.z) + c13.y);
    r0.xy = (r0.zx * c6.ww + c6.yy);
    r0 = tex2D(s1, r0.xy);
    r1.xyz = (-(input.t2.xyz) + c3.xyz);
    r1.x = dot(r1.xyz, r1.xyz);
    r1.x = rsqrt(r1.x);
    r1.x = (1.0 / r1.x);
    r1.x = (r1.x + -(c7.y));
    r1.x = saturate((r1.x * c1.x));
    r2.xyz = (-(input.t2.xyz) + c4.xyz);
    r2.w = dot(r2.xyz, r2.xyz);
    r2.w = rsqrt(r2.w);
    r2.xyz = (r2.xyz * r2.www);
    r2.w = (1.0 / r2.w);
    r3.xyz = normalize(input.t1.xyz);
    r1.y = dot(r3.xyz, r2.xyz);
    r1.z = (r2.w * -(c13.x) + c13.y);
    r1.y = saturate((r1.y * r1.z));
    r2 = (r1.yyyy * input.v0);
    r2 = (r2 * c5);
    r2 = (c0.xxxx >= 0 ? r2 : input.v0);
    r3 = lerp(r2, r0, r1.xxxx);
    r0 = (c2.xxxx >= 0 ? r3 : r2);
    r0.w = dot(c8, r0);
    r1.xyz = lerp(r0.xyz, r0.www, c12.yyy);
    r1.w = c12.x;
    r0.x = r1.x;
    r0.y = r1.w;
    r1.x = r1.y;
    r1.y = r1.w;
    r2.x = r1.z;
    r2.y = r1.w;
    r3 = tex2D(s0, r0.xy);
    r1 = tex2D(s0, r1.xy);
    r2 = tex2D(s0, r2.xy);
    r2.x = r3.x;
    r2.y = r1.y;
    r0.xyz = (r2.xyz * c13.www + c13.yyy);
    r0.xyz = (c9.yyy * r0.xyz + r2.xyz);
    r1.xyz = lerp(r0.xyz, r0.www, c9.xxx);
    r0.xyz = (r1.xyz + c9.zzz);
    r0.xyz = (r0.xyz + -(c9.www));
    r1.xyz = lerp(r0.xyz, c10.xyz, c10.www);
    r0.xyz = (r1.xyz + c11.xyz);
    r0.w = input.v0.w;
    output.oC0 = r0;
    return output;
}
