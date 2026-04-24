uniform sampler2D s0;

static const float4 c8 = float4(0.0005, 1.0, 0.5, -1.0);

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

    r0.w = (1.0 / input.t3.w);
    r0.x = (input.t3.x * r0.w + c8.y);
    r0.y = (input.t3.y * r0.w + c8.y);
    r0.x = (r0.x * c6.z);
    r0.z = c8.z;
    r0.z = (r0.x * r0.z + c6.x);
    r0.x = (r0.y * -(c8.z) + c8.y);
    r0.xy = (r0.zx * c6.ww + c6.yy);
    r0 = tex2D(s0, r0.xy);
    r1.xyz = (-(input.t2.xyz) + c3.xyz);
    r0.w = dot(r1.xyz, r1.xyz);
    r0.w = rsqrt(r0.w);
    r0.w = (1.0 / r0.w);
    r0.w = (r0.w + -(c7.y));
    r0.w = saturate((r0.w * c1.x));
    r1.xyz = (-(input.t2.xyz) + c4.xyz);
    r1.w = dot(r1.xyz, r1.xyz);
    r1.w = rsqrt(r1.w);
    r1.xyz = (r1.xyz * r1.www);
    r1.w = (1.0 / r1.w);
    r2.xyz = normalize(input.t1.xyz);
    r1.x = dot(r2.xyz, r1.xyz);
    r1.y = (r1.w * -(c8.x) + c8.y);
    r1.x = saturate((r1.x * r1.y));
    r1.xyz = (r1.xxx * input.v0.xyz);
    r1.xyz = (r1.xyz * c5.xyz);
    r1.xyz = (c0.xxx >= 0 ? r1.xyz : input.v0.xyz);
    r2.xyz = lerp(r1.xyz, r0.xyz, r0.www);
    r0.xyz = (c2.xxx >= 0 ? r2.xyz : r1.xyz);
    r0.xyz = (r0.xyz + c8.www);
    r0.xyz = (input.v0.www * r0.xyz + c8.yyy);
    r0.w = input.v0.w;
    output.oC0 = r0;
    return output;
}
